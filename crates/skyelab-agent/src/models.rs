//! The models and effort levels the composer's picker offers.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

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
    let mut child = cmd.spawn().context("starting codex to list its models")?;
    let result = (|| {
        let mut stdin = child.stdin.take().context("codex stdin was piped")?;
        let stdout = child.stdout.take().context("codex stdout was piped")?;
        let lines = [
            json!({ "id": 0, "method": "initialize", "params": { "clientInfo": {
                "name": "skyelab", "title": "SkyeLab", "version": env!("CARGO_PKG_VERSION") } } }),
            json!({ "method": "initialized", "params": {} }),
            json!({ "id": 1, "method": "model/list", "params": {} }),
        ];
        for l in lines {
            writeln!(stdin, "{l}").context("writing to codex")?;
        }
        stdin.flush().context("writing to codex")?;
        // The reader runs on its own thread, so a codex that never answers can't hold this past the deadline.
        let (tx, rx) = flume::bounded(1);
        std::thread::spawn(move || {
            let reply = BufReader::new(stdout)
                .lines()
                .map_while(std::result::Result::ok)
                .filter_map(|l| serde_json::from_str::<Value>(&l).ok())
                .find(|v| v["id"] == 1);
            let _ = tx.send(reply);
        });
        match rx.recv_timeout(LIST_DEADLINE) {
            Ok(Some(v)) => parse_codex_models(&v),
            Ok(None) => bail!("codex closed before listing its models"),
            Err(_) => bail!("codex didn't list its models in time"),
        }
    })();
    // shortcut: kills the leader only (on Windows, the npm shim); app-server starts nothing before a turn.
    let _ = child.kill();
    let _ = child.wait();
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
