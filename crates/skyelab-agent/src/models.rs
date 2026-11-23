//! The models and effort levels the composer's picker offers.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

/// One model the picker lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// What the CLI takes (`--model`, Codex's `model`).
    pub id: String,
    pub label: String,
    /// The efforts it accepts, lowest first. Empty: no effort choice.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

/// The aliases `claude --model` takes for the latest of each family (`claude --help`).
pub const CLAUDE_MODELS: &[(&str, &str)] = &[
    ("opus", "Opus"),
    ("sonnet", "Sonnet"),
    ("haiku", "Haiku"),
    ("fable", "Fable"),
];

/// `claude --effort` levels (`claude --help`).
pub const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// A model or effort name safe on a command line: id characters only, never flag-like.
pub(crate) fn valid_name(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && !v.starts_with('-')
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.:[]/".contains(c))
}

/// The most `model/list` pages read.
const MAX_PAGES: u64 = 10;

/// How long `codex app-server` gets to list its models.
const LIST_DEADLINE: Duration = Duration::from_secs(10);

/// The models Codex offers `account`, from `codex app-server`'s `model/list`, run under the account's own
/// `CODEX_HOME` in `dir` (which must resolve to it). `bin` is `codex` or a fake.
pub fn list_codex_models(
    bin: &Path,
    config: &AccountsConfig,
    account: &Account,
    dir: &Path,
) -> Result<Vec<ModelInfo>> {
    let (mut cmd, _, dir) = crate::launch::agent_command(bin, config, account, dir)?;
    cmd.args(crate::codex::args(&dir)?)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child =
        skyelab_core::process::spawn(&mut cmd).context("starting codex to list its models")?;
    let pid = child.id();
    let result = (|| {
        let mut stdin = child.stdin.take().context("codex stdin was piped")?;
        let stdout = child.stdout.take().context("codex stdout was piped")?;
        // Every reply goes to a channel from its own thread, so a codex that never answers can't hold this past the
        // deadline.
        let (tx, rx) = flume::unbounded();
        std::thread::spawn(move || {
            for v in BufReader::new(stdout)
                .lines()
                .map_while(std::result::Result::ok)
                .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
                .filter(|v| v.get("id").is_some() && v.get("method").is_none())
            {
                if tx.send(v).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + LIST_DEADLINE;
        let mut send = |v: Value| -> Result<()> {
            writeln!(stdin, "{v}").context("writing to codex")?;
            stdin.flush().context("writing to codex")
        };
        send(
            json!({ "id": 0, "method": "initialize", "params": crate::codex::initialize_params() }),
        )?;
        send(json!({ "method": "initialized", "params": {} }))?;
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        for page in 1..=MAX_PAGES {
            let params = if cursor.is_null() {
                json!({})
            } else {
                json!({ "cursor": cursor })
            };
            send(json!({ "id": page, "method": "model/list", "params": params }))?;
            let reply = loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(v) if v["id"] == page => break v,
                    Ok(_) => continue, // initialize's reply
                    Err(flume::RecvTimeoutError::Disconnected) => {
                        bail!("codex closed before listing its models")
                    }
                    Err(flume::RecvTimeoutError::Timeout) => {
                        bail!("codex didn't list its models in time")
                    }
                }
            };
            models.extend(parse_codex_models(&reply)?);
            cursor = reply["result"]["nextCursor"].clone();
            if !cursor.is_string() {
                break;
            }
        }
        Ok(models)
    })();
    #[cfg(unix)]
    crate::launch::kill_group(pid);
    // shortcut: off unix this kills the leader only (the npm shim); app-server starts nothing before a turn.
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(not(unix))]
    let _ = pid;
    result
}

/// `model/list`'s reply: `result.data[]` with `model`, `displayName`, `supportedReasoningEfforts[].reasoningEffort`
/// and `defaultReasoningEffort`.
fn parse_codex_models(v: &Value) -> Result<Vec<ModelInfo>> {
    if let Some(e) = v["error"]["message"].as_str() {
        bail!("codex refused to list its models: {e}");
    }
    let data = v["result"]["data"]
        .as_array()
        .context("codex's model list has no data")?;
    Ok(data
        .iter()
        .filter_map(|m| {
            let id = m["model"].as_str().filter(|id| valid_name(id))?;
            let efforts = m["supportedReasoningEfforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["reasoningEffort"].as_str())
                .filter(|e| valid_name(e))
                .map(str::to_owned)
                .collect();
            Some(ModelInfo {
                id: id.to_owned(),
                label: m["displayName"].as_str().unwrap_or(id).to_owned(),
                efforts,
                default_effort: m["defaultReasoningEffort"].as_str().map(str::to_owned),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_look_like_flags_are_refused() {
        assert!(valid_name("opus"));
        assert!(valid_name("claude-opus-5-5[1m]"));
        assert!(valid_name("gpt-5.5"));
        assert!(!valid_name("--dangerously-skip-permissions"));
        assert!(!valid_name("a b"));
        assert!(!valid_name(""));
    }

    #[test]
    fn codex_list_keeps_valid_models() {
        let v = json!({ "id": 1, "result": { "data": [
            { "model": "gpt-5.5", "displayName": "GPT-5.5", "defaultReasoningEffort": "medium",
              "supportedReasoningEfforts": [{ "reasoningEffort": "low" }, { "reasoningEffort": "medium" }] },
            { "model": "-x", "displayName": "bad" },
        ] } });
        let models = parse_codex_models(&v).expect("parse");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "GPT-5.5");
        assert_eq!(models[0].efforts, ["low", "medium"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert!(parse_codex_models(&json!({ "id": 1, "error": { "message": "no" } })).is_err());
    }
}
