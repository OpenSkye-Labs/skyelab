//! Whether an account is signed in to its agent CLI: `claude auth status` or `codex login status`, run with that
//! account's launch env only, so the answer is about its own config dir.

use anyhow::{Context as _, Result, bail};
use skyelab_core::{Account, Provider};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

const DEADLINE: Duration = Duration::from_secs(10);

/// What the CLI said about the account's login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Login {
    /// Signed in, with who or how when the CLI says (an email, "ChatGPT", "an API key").
    SignedIn(Option<String>),
    SignedOut,
}

/// Asks `bin` (the account's agent CLI) whether `account` is signed in. Blocking: call it off the UI thread.
pub fn login_status(bin: &Path, account: &Account) -> Result<Login> {
    // Codex and Cursor refuse/empty a config dir that doesn't exist, and nobody has logged in to a folder that isn't there.
    if matches!(account.provider, Provider::Codex | Provider::Cursor)
        && !account.config_dir.is_dir()
    {
        return Ok(Login::SignedOut);
    }
    skyelab_core::login_shell::wait();
    let resolved = if account.provider == Provider::Cursor {
        crate::cursor::resolve_cursor_bin(bin)?
    } else {
        crate::launch::resolve_bin(bin)?
    };
    let args: &[&str] = match account.provider {
        Provider::Claude => &["auth", "status"],
        Provider::Codex => &["login", "status"],
        Provider::Cursor => &["status"],
    };
    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .current_dir(std::env::temp_dir()) // never an untrusted project folder
        .env_clear()
        .envs(skyelab_core::env::inherited_agent_env(false))
        .envs(account.launch_env()?);
    let out = skyelab_core::process::output_within(&mut cmd, None, DEADLINE)
        .with_context(|| format!("running {} {}", resolved.display(), args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    match account.provider {
        Provider::Claude => parse_claude(&stdout, &stderr),
        Provider::Codex => parse_codex(&format!("{stdout}\n{stderr}"), out.status.success()),
        Provider::Cursor => parse_cursor(&format!("{stdout}\n{stderr}"), out.status.success()),
    }
}

/// `claude auth status` prints JSON (`loggedIn`, `email`, `orgName`, …) whether or not it is signed in. Claude Code
/// 2.1.331 pretty-prints it over many lines; older ones printed one line.
fn parse_claude(stdout: &str, stderr: &str) -> Result<Login> {
    // The JSON starts at the last line that opens an object in column 0: a notice may come before it, and a nested
    // object's lines are indented.
    let start = stdout
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .chain([0])
        .filter(|&i| stdout[i..].starts_with('{'))
        .max();
    let json = start.and_then(|i| {
        serde_json::Deserializer::from_str(&stdout[i..])
            .into_iter::<serde_json::Value>()
            .next()?
            .ok()
    });
    let Some(json) = json else {
        bail!("claude auth status: {}", first_line(stderr, stdout));
    };
    match json["loggedIn"].as_bool() {
        Some(true) => Ok(Login::SignedIn(
            ["email", "orgName"]
                .iter()
                .find_map(|k| json[k].as_str().filter(|s| !s.is_empty()))
                .map(str::to_owned),
        )),
        Some(false) => Ok(Login::SignedOut),
        None => bail!("claude auth status: no loggedIn in its answer"),
    }
}

/// `codex login status` says "Logged in using ChatGPT" (exit 0) or "Not logged in" (exit 1), on stderr. An API key
/// is printed half-masked after "API key"; only the method is kept.
fn parse_codex(text: &str, success: bool) -> Result<Login> {
    let line = text
        .lines()
        .find(|l| l.contains("Logged in") || l.to_ascii_lowercase().contains("not logged in"))
        .map_or_else(|| first_line(text, ""), str::to_owned);
    if success {
        let how = if line.contains("ChatGPT") {
            Some("ChatGPT")
        } else if line.contains("API key") {
            Some("an API key")
        } else {
            None
        };
        return Ok(Login::SignedIn(how.map(str::to_owned)));
    }
    if line.to_ascii_lowercase().contains("not logged in") {
        return Ok(Login::SignedOut);
    }
    bail!("codex login status: {line}")
}

/// `cursor-agent status` prints "Logged in as <email>" (exit 0) or "Not logged in" (exit 1).
fn parse_cursor(text: &str, success: bool) -> Result<Login> {
    let plain = crate::models::strip_ansi(text);
    if plain.to_ascii_lowercase().contains("not logged in") {
        return Ok(Login::SignedOut);
    }
    let line = plain
        .lines()
        .find(|l| l.to_ascii_lowercase().contains("logged in"))
        .map_or_else(|| first_line(&plain, ""), str::to_owned);
    if success {
        let who = line
            .split_once("Logged in as ")
            .or_else(|| line.split_once("logged in as "))
            .map(|(_, rest)| rest.trim())
            .map(|rest| rest.split(" (").next().unwrap_or(rest).trim())
            .map(|rest| rest.chars().filter(|c| !c.is_control()).collect::<String>())
            .filter(|s| !s.is_empty());
        return Ok(Login::SignedIn(who));
    }
    bail!("cursor-agent status: {line}")
}

fn first_line(a: &str, b: &str) -> String {
    a.lines()
        .chain(b.lines())
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("no output")
        .chars()
        .take(160)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Login, parse_claude, parse_codex};

    #[test]
    fn claude_status_json() {
        let on = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"me@example.com","orgName":"Acme"}"#;
        assert_eq!(
            parse_claude(on, "").expect("parsed"),
            Login::SignedIn(Some("me@example.com".into()))
        );
        let org = r#"{"loggedIn":true,"authMethod":"console","orgName":"Acme"}"#;
        assert_eq!(
            parse_claude(org, "").expect("parsed"),
            Login::SignedIn(Some("Acme".into()))
        );
        assert_eq!(
            parse_claude(r#"{"loggedIn":true,"authMethod":"apiKey"}"#, "").expect("parsed"),
            Login::SignedIn(None)
        );
        assert_eq!(
            parse_claude("{\"loggedIn\":false}\n", "").expect("parsed"),
            Login::SignedOut
        );
        assert_eq!(
            parse_claude("Update available: 2.2.0\n{\"loggedIn\":false}\n", "").expect("parsed"),
            Login::SignedOut,
            "a notice before the JSON"
        );
        // Claude Code 2.1.331's real answer (private values replaced): pretty-printed over many lines.
        let real = "{\n  \"loggedIn\": true,\n  \"authMethod\": \"claude.ai\",\n  \"apiProvider\": \"firstParty\",\n  \"configDirectory\": \"/Users/me/.claude-work\",\n  \"loginExpiresAt\": \"2026-12-20T02:59:27.942Z\",\n  \"email\": \"me@example.com\",\n  \"orgId\": \"00000000-0000-0000-0000-000000000000\",\n  \"orgName\": \"Acme\",\n  \"subscriptionType\": \"team\",\n  \"storedClaudeAiLogin\": true,\n  \"planTier\": \"other\"\n}\n";
        assert_eq!(
            parse_claude(real, "").expect("parsed"),
            Login::SignedIn(Some("me@example.com".into()))
        );
        assert_eq!(
            parse_claude(
                "Update available\n{\n  \"loggedIn\": false,\n  \"x\": {\n    \"y\": 1\n  }\n}\n",
                ""
            )
            .expect("parsed"),
            Login::SignedOut,
            "a notice, then pretty JSON with a nested object"
        );
        let unknown = parse_claude("", "error: unknown command 'auth'").expect_err("not an answer");
        assert!(unknown.to_string().contains("unknown command"), "{unknown}");
    }

    #[test]
    fn codex_status_text() {
        assert_eq!(
            parse_codex("\nLogged in using ChatGPT\n", true).expect("parsed"),
            Login::SignedIn(Some("ChatGPT".into()))
        );
        let key =
            parse_codex("Logged in using an API key - sk-proj-***ABCD", true).expect("parsed");
        assert_eq!(
            key,
            Login::SignedIn(Some("an API key".into())),
            "the key never shows"
        );
        assert_eq!(
            parse_codex("Not logged in\n", false).expect("parsed"),
            Login::SignedOut
        );
        assert_eq!(
            parse_codex("WARNING: proceeding, even though…\nNot logged in\n", false)
                .expect("parsed"),
            Login::SignedOut,
            "a warning before the answer"
        );
        assert!(parse_codex("Error loading configuration: bad toml", false).is_err());
    }
}

#[cfg(all(test, unix))]
mod fake_cli_tests {
    use super::{Login, login_status};
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

    #[test]
    fn asks_each_account_about_its_own_config_dir() {
        let home = tempfile::tempdir().expect("tempdir");
        let (a, b) = (home.path().join("a"), home.path().join("b"));
        std::fs::create_dir_all(&a).expect("mkdir");
        std::fs::create_dir_all(&b).expect("mkdir");
        std::fs::write(a.join("fake-login"), "a@example.com\n").expect("sign a in");
        let claude = fixture("../skyelab-app/tests/fixtures/fake-claude.sh");
        assert_eq!(
            login_status(&claude, &account(Provider::Claude, &a)).expect("status"),
            Login::SignedIn(Some("a@example.com".into()))
        );
        assert_eq!(
            login_status(&claude, &account(Provider::Claude, &b)).expect("status"),
            Login::SignedOut,
            "b's own dir has no login, whatever a's has"
        );
        let codex = fixture("tests/fixtures/fake-codex.sh");
        assert_eq!(
            login_status(&codex, &account(Provider::Codex, &home.path().join("none")))
                .expect("status"),
            Login::SignedOut,
            "a Codex folder not made yet: signed out, not an error"
        );
        assert_eq!(
            login_status(&codex, &account(Provider::Codex, &b)).expect("status"),
            Login::SignedOut
        );
        std::fs::write(b.join("fake-login"), "").expect("sign b in");
        assert_eq!(
            login_status(&codex, &account(Provider::Codex, &b)).expect("status"),
            Login::SignedIn(Some("ChatGPT".into()))
        );
    }
}
