//! An account's usage limits, asked of its own agent CLI between turns (docs/decisions/agents.md, "Usage limits
//! are asked of the account's CLI"). Claude: a `get_usage` control request on `claude -p` stream-json, as T3 does
//! through the Agent SDK. Codex: `account/rateLimits/read` on `codex app-server`, as T3 does. The CLI reads its own
//! login and calls its own endpoint, so SkyeLab never sees a token; it runs with the account's launch env only.

use crate::Event;
use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, Provider};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_secs(20);
const REQUEST_ID: &str = "skyelab-usage";

/// What the account's CLI said about its limits.
#[derive(Debug, Clone, PartialEq)]
pub enum Limits {
    /// `RateLimit` events, one per window (and, for Codex, its limited summary last), as the agent reports them in
    /// a turn.
    Readings(Vec<Event>),
    /// No limits to read: signed out, an API key, or a plan without windows.
    Unavailable,
}

/// Asks `bin` (the account's agent CLI) for `account`'s usage windows. Blocking: call it off the UI thread. An error
/// says only what went wrong in SkyeLab's words, never the CLI's text.
pub fn usage_limits(bin: &Path, account: &Account) -> Result<Limits> {
    if account.provider == Provider::Cursor {
        return Ok(Limits::Unavailable);
    }
    if account.provider == Provider::Codex && !account.config_dir.is_dir() {
        return Ok(Limits::Unavailable);
    }
    skyelab_core::login_shell::wait();
    let resolved = crate::launch::resolve_bin(bin)?;
    let mut cmd = Command::new(&resolved);
    let lines = match account.provider {
        // No settings (their hooks would run every poll), no MCP servers: the probe only reads limits.
        Provider::Claude => {
            cmd.args([
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--setting-sources",
                "",
                "--strict-mcp-config",
            ]);
            vec![json!({
                "type": "control_request",
                "request_id": REQUEST_ID,
                "request": { "subtype": "get_usage", "skip_behaviors": true },
            })]
        }
        Provider::Codex => {
            cmd.arg("app-server");
            vec![
                json!({ "id": 1, "method": "initialize", "params": crate::codex::initialize_params() }),
                json!({ "method": "initialized", "params": {} }),
                json!({ "id": 2, "method": "account/rateLimits/read", "params": null }),
            ]
        }
        Provider::Cursor => return Ok(Limits::Unavailable),
    };
    cmd.current_dir(std::env::temp_dir()) // never an untrusted project folder
        .env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false))
        .envs(account.launch_env()?);
    if account.provider == Provider::Claude {
        cmd.env(
            crate::launch::NO_CLAUDEAI_MCP.0,
            crate::launch::NO_CLAUDEAI_MCP.1,
        );
    }
    let answer = ask(&mut cmd, &lines, account.provider)?;
    match account.provider {
        Provider::Claude => parse_claude(&answer),
        Provider::Codex => parse_codex(&answer),
        Provider::Cursor => Ok(Limits::Unavailable),
    }
}

/// Writes `lines` to the child's stdin and keeps it open (Codex answers only while it is), until stdout has the
/// answer line or [`DEADLINE`] passes. The child is killed either way when its handle drops.
fn ask(cmd: &mut Command, lines: &[Value], provider: Provider) -> Result<Value> {
    let mut child = skyelab_core::process::spawn(
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()),
    )
    .context("starting the agent CLI")?;
    let (mut stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
        (Some(i), Some(o)) => (i, o),
        _ => bail!("the agent CLI has no pipes"),
    };
    let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if let Err(e) = stdin
        .write_all(input.as_bytes())
        .and_then(|()| stdin.flush())
    {
        // A CLI that quit before reading its input is the same case as one that quit without answering.
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            bail!("the agent CLI exited without answering");
        }
        return Err(e).context("writing to the agent CLI");
    }
    let (tx, rx) = flume::bounded(1);
    // Ends at EOF, once the child exits or is killed.
    std::thread::spawn(move || {
        let found = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
            .find(|v| is_answer(v, provider));
        let _ = tx.send(found);
    });
    let start = Instant::now();
    match rx.recv_timeout(DEADLINE) {
        Ok(Some(v)) => Ok(v),
        Ok(None) => bail!("the agent CLI exited without answering"),
        Err(_) => bail!(
            "the agent CLI didn't answer within {}s",
            start.elapsed().as_secs()
        ),
    }
}

fn is_answer(v: &Value, provider: Provider) -> bool {
    match provider {
        Provider::Claude => {
            v["type"] == "control_response" && v["response"]["request_id"] == REQUEST_ID
        }
        Provider::Codex => v["id"] == 2,
        Provider::Cursor => false,
    }
}

/// Claude's `get_usage` answer: `rate_limits.five_hour` and `.seven_day`, `utilization` 0 to 100 and `resets_at`
/// an ISO time. No limited summary: whether overage still lets the agent work isn't in it, so "Limited" stays what
/// the agent reported in a turn.
fn parse_claude(v: &Value) -> Result<Limits> {
    let r = &v["response"];
    if r["subtype"] != "success" {
        bail!("Claude refused the usage request");
    }
    let r = &r["response"];
    if r["rate_limits_available"] != true {
        return Ok(Limits::Unavailable);
    }
    let limits = &r["rate_limits"];
    let readings: Vec<Event> = ["five_hour", "seven_day"]
        .into_iter()
        .filter_map(|w| {
            let used = limits[w]["utilization"]
                .as_f64()
                .filter(|u| u.is_finite())?;
            Some(Event::RateLimit {
                limited: false,
                resets_at: limits[w]["resets_at"]
                    .as_str()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
                    .map(|t| t.timestamp()),
                window: Some(w.to_owned()),
                used: Some((used / 100.0).clamp(0.0, 1.0) as f32),
            })
        })
        .collect();
    if readings.is_empty() {
        // Available but empty: the CLI's own read failed (a 429, a network error).
        bail!("Claude had no usage to report");
    }
    Ok(Limits::Readings(readings))
}

/// Codex's `account/rateLimits/read` answer: `rateLimits`, the shape of its `account/rateLimits/updated`
/// notification. Its refusal for a signed-out account says "authentication required".
fn parse_codex(v: &Value) -> Result<Limits> {
    if let Some(message) = v["error"]["message"].as_str() {
        if message.contains("authentication") {
            return Ok(Limits::Unavailable);
        }
        bail!("Codex refused the usage request");
    }
    let readings = crate::codex::rate_limit(&v["result"]["rateLimits"]);
    match readings.iter().any(|e| e.rate_limit().is_some()) {
        true => Ok(Limits::Readings(readings)),
        false => Ok(Limits::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::{Limits, parse_claude, parse_codex};
    use crate::Event;
    use serde_json::json;

    fn windows(u: &Limits) -> Vec<(&'static str, f32)> {
        match u {
            Limits::Readings(r) => r.iter().filter_map(Event::rate_limit).collect(),
            Limits::Unavailable => Vec::new(),
        }
    }

    #[test]
    fn claude_answer() {
        let ok = json!({"type":"control_response","response":{"subtype":"success","request_id":"skyelab-usage","response":{
            "rate_limits_available":true,
            "rate_limits":{"five_hour":{"utilization":42.0,"resets_at":"2026-11-25T18:00:00Z"},"seven_day":{"utilization":130,"resets_at":null}}}}});
        let u = parse_claude(&ok).expect("parsed");
        assert_eq!(windows(&u), vec![("5h", 0.42), ("Weekly", 1.0)]);
        let Limits::Readings(r) = u else {
            unreachable!()
        };
        assert!(
            matches!(
                r[0],
                Event::RateLimit {
                    resets_at: Some(1_795_629_600),
                    limited: false,
                    ..
                }
            ),
            "{r:?}"
        );
        assert!(
            r.iter().all(|e| matches!(
                e,
                Event::RateLimit {
                    window: Some(_),
                    ..
                }
            )),
            "no summary: a poll never says limited or lifts it"
        );
        let signed_out = json!({"type":"control_response","response":{"subtype":"success","request_id":"skyelab-usage","response":{"rate_limits_available":false,"rate_limits":null}}});
        assert_eq!(
            parse_claude(&signed_out).expect("parsed"),
            Limits::Unavailable
        );
        let failed = json!({"type":"control_response","response":{"subtype":"success","request_id":"skyelab-usage","response":{"rate_limits_available":true,"rate_limits":null}}});
        assert!(
            parse_claude(&failed).is_err(),
            "a failed read is an error, not no limits"
        );
        let refused = json!({"type":"control_response","response":{"subtype":"error","request_id":"skyelab-usage","error":"secret detail"}});
        let e = parse_claude(&refused).expect_err("refused");
        assert!(
            !e.to_string().contains("secret"),
            "the CLI's text stays out: {e}"
        );
    }

    #[test]
    fn codex_answer() {
        let ok = json!({"id":2,"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":61,"windowDurationMins":300,"resetsAt":1_777_429_830},"secondary":{"usedPercent":23,"windowDurationMins":10080,"resetsAt":1_777_959_590},"rateLimitReachedType":null}}});
        let u = parse_codex(&ok).expect("parsed");
        assert_eq!(windows(&u), vec![("5h", 0.61), ("Weekly", 0.23)]);
        let signed_out = json!({"id":2,"error":{"code":-32600,"message":"codex account authentication required to read rate limits"}});
        assert_eq!(
            parse_codex(&signed_out).expect("parsed"),
            Limits::Unavailable
        );
        let other = json!({"id":2,"error":{"code":-32603,"message":"token sk-secret expired"}});
        let e = parse_codex(&other).expect_err("refused");
        assert!(!e.to_string().contains("sk-secret"), "{e}");
    }
}

#[cfg(all(test, unix))]
mod fake_cli_tests {
    use super::{Limits, usage_limits};
    use crate::Event;
    use skyelab_core::{Account, Provider};
    use std::path::{Path, PathBuf};

    fn account(provider: Provider, dir: &Path) -> Account {
        Account {
            id: "work".into(),
            label: "Work".into(),
            provider,
            config_dir: dir.to_owned(),
            gh_config_dir: None,
            ssh_agent: false,
        }
    }

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)
    }

    fn windows(u: &Limits) -> Vec<(&'static str, f32)> {
        match u {
            Limits::Readings(r) => r.iter().filter_map(Event::rate_limit).collect(),
            Limits::Unavailable => Vec::new(),
        }
    }

    #[test]
    fn asks_each_account_for_its_own_limits() {
        let home = tempfile::tempdir().expect("tempdir");
        let (a, b) = (home.path().join("a"), home.path().join("b"));
        std::fs::create_dir_all(&a).expect("mkdir");
        std::fs::create_dir_all(&b).expect("mkdir");
        std::fs::write(
            a.join("fake-usage.json"),
            r#"{"rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":42,"resets_at":"2026-11-25T18:00:00Z"},"seven_day":{"utilization":17,"resets_at":"2026-11-30T18:00:00Z"}}}"#,
        )
        .expect("a's usage");
        let claude = fixture("../skyelab-app/tests/fixtures/fake-claude.sh");
        let got = usage_limits(&claude, &account(Provider::Claude, &a)).expect("usage");
        assert_eq!(windows(&got), vec![("5h", 0.42), ("Weekly", 0.17)]);
        assert_eq!(
            usage_limits(&claude, &account(Provider::Claude, &b)).expect("usage"),
            Limits::Unavailable,
            "b's own dir has no usage, whatever a's has"
        );

        let codex = fixture("tests/fixtures/fake-codex.sh");
        assert_eq!(
            usage_limits(&codex, &account(Provider::Codex, &b)).expect("usage"),
            Limits::Unavailable,
            "signed out"
        );
        std::fs::write(
            b.join("fake-usage.json"),
            r#"{"rateLimits":{"limitId":"codex","primary":{"usedPercent":47,"windowDurationMins":300,"resetsAt":2000000000},"secondary":{"usedPercent":83,"windowDurationMins":10080,"resetsAt":2000100000},"rateLimitReachedType":null}}"#,
        )
        .expect("b's usage");
        let got = usage_limits(&codex, &account(Provider::Codex, &b)).expect("usage");
        assert_eq!(windows(&got), vec![("5h", 0.47), ("Weekly", 0.83)]);
    }

    #[test]
    fn a_cli_that_never_answers_times_out_or_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let silent = dir.path().join("silent.sh");
        std::fs::write(&silent, "#!/bin/sh\nexit 0\n").expect("script");
        let mut perms = std::fs::metadata(&silent).expect("meta").permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&silent, perms).expect("chmod");
        let e =
            usage_limits(&silent, &account(Provider::Claude, dir.path())).expect_err("no answer");
        assert!(e.to_string().contains("without answering"), "{e}");
    }
}
