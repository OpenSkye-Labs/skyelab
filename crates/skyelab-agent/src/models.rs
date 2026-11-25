//! The models and effort levels the composer's picker offers.

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use skyelab_core::{Account, AccountsConfig, Provider};
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
    /// The one `model/list` marks as the account's default.
    pub is_default: bool,
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

/// A model id as people say it: `claude-opus-5-5` is "Opus 5.5", `gpt-5.5` "GPT-5.5", a `[1m]` suffix " (1M)". An id
/// it doesn't recognise stays as it is.
pub fn model_label(id: &str) -> String {
    let (base, wide) = match id.strip_suffix("[1m]") {
        Some(base) => (base, " (1M)"),
        None => (id, ""),
    };
    let family = |f: &str| CLAUDE_MODELS.iter().find(|(a, _)| *a == f).map(|(_, l)| *l);
    let name = if let Some(label) = family(base) {
        label.to_owned()
    } else if let Some(rest) = base.strip_prefix("claude-") {
        // `claude-opus-5-5`, `claude-sonnet-4-5-20250929`, `claude-3-5-sonnet-20241022`: a family plus version numbers.
        let parts: Vec<&str> = rest
            .split('-')
            .filter(|p| !(p.len() == 8 && p.bytes().all(|b| b.is_ascii_digit())))
            .collect();
        let named = parts.iter().find_map(|p| family(p));
        let version: Vec<&str> = parts
            .iter()
            .copied()
            .filter(|p| family(p).is_none())
            .collect();
        match named {
            Some(f)
                if !version.is_empty()
                    && version
                        .iter()
                        .all(|p| p.bytes().all(|b| b.is_ascii_digit())) =>
            {
                format!("{f} {}", version.join("."))
            }
            _ => return id.to_owned(),
        }
    } else if let Some(rest) = base.strip_prefix("gpt-") {
        let words: Vec<String> = rest
            .split('-')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            })
            .collect();
        format!("GPT-{}", words.join("-"))
    } else {
        return id.to_owned();
    };
    format!("{name}{wide}")
}

/// The most of an account's config file read for its default model.
const MAX_CONFIG: u64 = 256 * 1024;

/// The model `account` runs when none is picked, from its own config dir: Claude's `settings.json` `model`, Codex's
/// `config.toml` top-level `model`, Cursor's `cli-config.json` `model.modelId` (or `model`). Reads that one file,
/// keeps only that field (credentials and the rest are never looked at), and never writes. `None` when it isn't set
/// or isn't a plain model name.
pub fn account_default_model(account: &Account) -> Option<String> {
    use std::io::Read as _;
    #[derive(serde::Deserialize)]
    struct OnlyModel {
        model: Option<String>,
    }
    let file = match account.provider {
        Provider::Claude => "settings.json",
        Provider::Codex => "config.toml",
        Provider::Cursor => "cli-config.json",
        Provider::Antigravity => return None,
    };
    let path = account.config_dir.join(file);
    // A regular file only: a FIFO or device there would block the read forever.
    if !std::fs::metadata(&path).ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .ok()?
        .take(MAX_CONFIG)
        .read_to_string(&mut text)
        .ok()?;
    let model = match account.provider {
        Provider::Claude => serde_json::from_str::<OnlyModel>(&text).ok()?.model,
        Provider::Codex => toml::from_str::<OnlyModel>(&text).ok()?.model,
        Provider::Cursor => {
            let v: Value = serde_json::from_str(&text).ok()?;
            v["model"]["modelId"]
                .as_str()
                .or_else(|| v["model"].as_str())
                .map(str::to_owned)
        }
        Provider::Antigravity => return None,
    };
    model.filter(|m| valid_name(m))
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

/// The models Antigravity offers `account`, from `agy acp`'s `session/new` `configOptions` (or legacy
/// `models.availableModels`), run under the account's own `GEMINI_HOME` in `dir` (which must resolve to it).
pub fn list_antigravity_models(
    bin: &Path,
    config: &AccountsConfig,
    account: &Account,
    dir: &Path,
) -> Result<Vec<ModelInfo>> {
    let (mut cmd, _, dir) = crate::launch::agent_command(bin, config, account, dir)?;
    crate::antigravity::ensure_profile_dirs(&account.config_dir)?;
    cmd.envs(crate::antigravity::isolated_env())
        .args(crate::antigravity::acp_args(&account.config_dir, false));
    crate::acp_probe::list_models(
        &mut cmd,
        &crate::antigravity::config(),
        &dir,
        Duration::from_secs(15),
    )
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
                is_default: m["isDefault"].as_bool().unwrap_or(false),
            })
        })
        .collect())
}

/// The most bytes read from `cursor-agent models` stdout.
const MAX_CURSOR_MODELS_BYTES: usize = 1 << 20;
/// The most models kept from `cursor-agent models`.
const MAX_CURSOR_MODELS: usize = 256;

/// The models Cursor offers `account`, from `cursor-agent models`, run under the account's own
/// `CURSOR_CONFIG_DIR` in `dir` (which must resolve to it). `bin` is `cursor-agent` or a fake.
pub fn list_cursor_models(
    bin: &Path,
    config: &AccountsConfig,
    account: &Account,
    dir: &Path,
) -> Result<Vec<ModelInfo>> {
    let (mut cmd, _, dir) = crate::launch::agent_command(bin, config, account, dir)?;
    cmd.arg("models").current_dir(&dir);
    let mut run = skyelab_core::process::Captured::spawn(&mut cmd, None)
        .context("starting cursor-agent models")?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = run
            .child
            .try_wait()
            .context("waiting for cursor-agent models")?
        {
            break status;
        }
        if start.elapsed() > LIST_DEADLINE {
            #[cfg(unix)]
            crate::launch::kill_group(run.child.id());
            let _ = run.child.kill();
            let _ = run.child.wait();
            bail!(
                "cursor didn't list its models within {}s",
                LIST_DEADLINE.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let out = run
        .output_capped(status, MAX_CURSOR_MODELS_BYTES)
        .context("reading cursor-agent models output")?;
    if !status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let msg = stderr
            .lines()
            .chain(stdout.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("cursor-agent models failed");
        bail!("cursor refused to list its models: {msg}");
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(parse_cursor_models(&text))
}

/// Strips ANSI CSI escape sequences (e.g. `\x1b[32m`) from CLI output.
pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Parses `cursor-agent models` output (lines like `id - Label (default)` or `- id`, or a JSON list).
fn parse_cursor_models(text: &str) -> Vec<ModelInfo> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        let arr = v
            .as_array()
            .or_else(|| v["models"].as_array())
            .or_else(|| v["data"].as_array());
        if let Some(arr) = arr {
            let mut out = Vec::new();
            let mut seen = std::collections::HashSet::new();
            for item in arr {
                if out.len() >= MAX_CURSOR_MODELS {
                    break;
                }
                let id = item
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .or_else(|| item["modelId"].as_str())
                    .or_else(|| item["model"].as_str());
                let Some(id) = id.filter(|id| valid_name(id)) else {
                    continue;
                };
                if !seen.insert(id.to_owned()) {
                    continue;
                }
                let label = item["label"]
                    .as_str()
                    .or_else(|| item["displayName"].as_str())
                    .or_else(|| item["name"].as_str())
                    .unwrap_or(id)
                    .to_owned();
                let is_default = item["isDefault"]
                    .as_bool()
                    .or_else(|| item["default"].as_bool())
                    .unwrap_or(false);
                out.push(ModelInfo {
                    id: id.to_owned(),
                    label,
                    efforts: Vec::new(),
                    default_effort: None,
                    is_default,
                });
            }
            return out;
        }
    }
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in text.lines() {
        if out.len() >= MAX_CURSOR_MODELS {
            break;
        }
        let plain = strip_ansi(raw);
        let line = plain.trim();
        if line.is_empty() || line.ends_with(':') {
            continue;
        }
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line)
            .trim();
        let is_default = line.contains("(default)") || line.contains("(current)");
        let cleaned = line
            .replace("(default)", "")
            .replace("(current)", "")
            .trim()
            .to_owned();
        let (id, label) = match cleaned.split_once(" - ") {
            Some((id, label)) => (id.trim(), label.trim()),
            None => {
                // Only accept a single bare model token; skip multi-word prose/headers.
                if cleaned.is_empty() || cleaned.contains(char::is_whitespace) {
                    continue;
                }
                (cleaned.as_str(), cleaned.as_str())
            }
        };
        if !valid_name(id) || !seen.insert(id.to_owned()) {
            continue;
        }
        out.push(ModelInfo {
            id: id.to_owned(),
            label: if label.is_empty() {
                id.to_owned()
            } else {
                label.to_owned()
            },
            efforts: Vec::new(),
            default_effort: None,
            is_default,
        });
    }
    out
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
            { "model": "gpt-5.5", "displayName": "GPT-5.5", "defaultReasoningEffort": "medium", "isDefault": true,
              "supportedReasoningEfforts": [{ "reasoningEffort": "low" }, { "reasoningEffort": "medium" }] },
            { "model": "-x", "displayName": "bad" },
        ] } });
        let models = parse_codex_models(&v).expect("parse");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "GPT-5.5");
        assert_eq!(models[0].efforts, ["low", "medium"]);
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert!(models[0].is_default);
        assert!(parse_codex_models(&json!({ "id": 1, "error": { "message": "no" } })).is_err());
    }

    #[test]
    fn cursor_models_strip_ansi_and_skip_prose_lines() {
        let raw = "\x1b[1mAvailable models\x1b[0m\n\
                   Tip: pass --model <id>\n\
                   \x1b[32m  auto - Auto (default)\x1b[0m\n\
                   - composer-1\n\
                   --bad-flag - Ignored\n";
        let models = parse_cursor_models(raw);
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["auto", "composer-1"]);
        assert!(models[0].is_default);
    }
}
