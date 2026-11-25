//! Bounded ACP stdio probe helpers: `check_login`, `list_models`, `one_shot`, and MCP grant conversion.
//!
//! Every probe enforces a hard per-line byte cap ([`MAX_PROBE_LINE`]), a hard frame cap
//! ([`MAX_PROBE_FRAMES`]), and a wall-clock deadline, killing the child's process group on return.

use crate::Login;
use crate::acp::{
    AcpConfig, cancel_permission_reply, initialize_params, is_auth_required_error,
    rpc_error_message, valid_rpc_id,
};
use crate::acp_updates::{content_text, truncate_utf8};
use crate::models::{ModelInfo, valid_name};
use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Hard per-line byte cap for ACP probe / one-shot stdout.
pub(crate) const MAX_PROBE_LINE: usize = 4 << 20;
/// Hard cap on stdout frames read during a single ACP probe.
pub(crate) const MAX_PROBE_FRAMES: usize = 512;
const MAX_MODELS: usize = 256;
const MAX_EFFORTS: usize = 32;

/// Converts a SkyeLab `--mcp-config` JSON grant file into ACP `mcpServers` array format.
pub(crate) fn mcp_servers_from_grant(grant: &Path) -> Result<Vec<Value>> {
    let text =
        std::fs::read_to_string(grant).with_context(|| format!("reading {}", grant.display()))?;
    let v: Value = serde_json::from_str(&text).context("parsing the MCP grant")?;
    let s = &v["mcpServers"]["skyelab"];
    anyhow::ensure!(s["command"].is_string(), "the MCP grant names no command");
    let args = s["args"].as_array().cloned().unwrap_or_default();
    let env: Vec<Value> = s["env"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some(json!({ "name": k, "value": v.as_str()? })))
        .collect();
    Ok(vec![json!({
        "name": "skyelab",
        "command": s["command"],
        "args": args,
        "env": env,
    })])
}

/// Runs `initialize` -> `session/new` to check whether the ACP server is signed in.
pub(crate) fn check_login(
    cmd: &mut Command,
    cfg: &AcpConfig,
    cwd: &Path,
    deadline: Duration,
) -> Result<Login> {
    let reply = probe_session_new(cmd, cfg, cwd, deadline)?;
    if let Some(msg) = rpc_error_message(&reply) {
        let code = reply["error"]["code"].as_i64().unwrap_or(0);
        if is_auth_required_error(code, &msg) {
            return Ok(Login::SignedOut);
        }
        bail!("{} sign-in check failed: {msg}", cfg.provider_label);
    }
    if reply["result"]["sessionId"].is_string() {
        Ok(Login::SignedIn(Some("Google".into())))
    } else {
        bail!(
            "{} returned an unexpected session/new reply",
            cfg.provider_label
        )
    }
}

/// Runs `initialize` -> `session/new` and extracts models + `thought_level` efforts.
pub(crate) fn list_models(
    cmd: &mut Command,
    cfg: &AcpConfig,
    cwd: &Path,
    deadline: Duration,
) -> Result<Vec<ModelInfo>> {
    let reply = probe_session_new(cmd, cfg, cwd, deadline)?;
    if let Some(msg) = rpc_error_message(&reply) {
        let code = reply["error"]["code"].as_i64().unwrap_or(0);
        if is_auth_required_error(code, &msg) {
            bail!(
                "Sign in to {} in Settings → Accounts first.",
                cfg.provider_label
            );
        }
        bail!("{} refused to list models: {msg}", cfg.provider_label);
    }
    Ok(parse_models_from_session(&reply["result"]))
}

/// Runs a single prompt over ACP in a clean empty temp directory with `mcpServers: []` and auto-cancels
/// any `session/request_permission` so untrusted prompt text can never run tools.
pub(crate) fn one_shot(
    cmd: &mut Command,
    cfg: &AcpConfig,
    prompt: &str,
    deadline: Duration,
    max_output: usize,
) -> Result<String> {
    let scratch =
        tempfile::tempdir().context("creating empty scratch dir for one-shot ACP call")?;
    cmd.current_dir(scratch.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = skyelab_core::process::spawn(cmd).context("starting ACP one-shot")?;
    let pid = child.id();
    let result = (|| {
        let mut stdin = child.stdin.take().context("ACP stdin was piped")?;
        let stdout = child.stdout.take().context("ACP stdout was piped")?;
        let rx = spawn_bounded_reader(stdout, MAX_PROBE_LINE, MAX_PROBE_FRAMES);
        let limit = Instant::now() + deadline;

        send_json(
            &mut stdin,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": initialize_params(cfg) }),
        )?;
        let init = wait_response(&rx, &mut stdin, 1, limit, None, max_output)?;
        if let Some(msg) = rpc_error_message(&init) {
            bail!("{} initialize failed: {msg}", cfg.provider_label);
        }

        send_json(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "session/new",
                "params": { "cwd": scratch.path(), "mcpServers": [] },
            }),
        )?;
        let opened = wait_response(&rx, &mut stdin, 2, limit, None, max_output)?;
        if let Some(msg) = rpc_error_message(&opened) {
            bail!("{} session/new failed: {msg}", cfg.provider_label);
        }
        let session_id = opened["result"]["sessionId"]
            .as_str()
            .context("ACP session/new returned no sessionId")?
            .to_owned();

        send_json(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "session/prompt",
                "params": {
                    "sessionId": session_id,
                    "prompt": [{ "type": "text", "text": prompt }],
                },
            }),
        )?;
        let mut out = String::new();
        let done = wait_response(&rx, &mut stdin, 3, limit, Some(&mut out), max_output)?;
        if let Some(msg) = rpc_error_message(&done) {
            bail!("{} prompt failed: {msg}", cfg.provider_label);
        }
        Ok(out)
    })();
    reap_child(&mut child, pid);
    result
}

fn probe_session_new(
    cmd: &mut Command,
    cfg: &AcpConfig,
    cwd: &Path,
    deadline: Duration,
) -> Result<Value> {
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = skyelab_core::process::spawn(cmd).context("starting ACP probe")?;
    let pid = child.id();
    let result = (|| {
        let mut stdin = child.stdin.take().context("ACP stdin was piped")?;
        let stdout = child.stdout.take().context("ACP stdout was piped")?;
        let rx = spawn_bounded_reader(stdout, MAX_PROBE_LINE, MAX_PROBE_FRAMES);
        let limit = Instant::now() + deadline;

        send_json(
            &mut stdin,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": initialize_params(cfg) }),
        )?;
        let init = wait_response(&rx, &mut stdin, 1, limit, None, 0)?;
        if let Some(msg) = rpc_error_message(&init) {
            bail!("{} initialize failed: {msg}", cfg.provider_label);
        }

        send_json(
            &mut stdin,
            &json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "session/new",
                "params": { "cwd": cwd, "mcpServers": [] },
            }),
        )?;
        wait_response(&rx, &mut stdin, 2, limit, None, 0)
    })();
    reap_child(&mut child, pid);
    result
}

enum ProbeMsg {
    Json(Value),
    Overflow(&'static str),
}

fn spawn_bounded_reader(
    stdout: impl std::io::Read + Send + 'static,
    max_line: usize,
    max_frames: usize,
) -> flume::Receiver<ProbeMsg> {
    let (tx, rx) = flume::bounded(64);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        let mut frames = 0usize;
        loop {
            buf.clear();
            let take = max_line as u64 + 1;
            match (&mut reader).take(take).read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if buf.len() > max_line && buf.last() != Some(&b'\n') {
                let _ = tx.send(ProbeMsg::Overflow("ACP probe line exceeded byte limit"));
                break;
            }
            frames += 1;
            if frames > max_frames {
                let _ = tx.send(ProbeMsg::Overflow("ACP probe exceeded frame limit"));
                break;
            }
            let text = String::from_utf8_lossy(&buf);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(trimmed)
                && tx.send(ProbeMsg::Json(v)).is_err()
            {
                break;
            }
        }
    });
    rx
}

fn wait_response(
    rx: &flume::Receiver<ProbeMsg>,
    stdin: &mut std::process::ChildStdin,
    wanted_id: u64,
    deadline: Instant,
    mut collect_text: Option<&mut String>,
    max_output: usize,
) -> Result<Value> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            bail!("ACP process did not answer in time");
        }
        match rx.recv_timeout(left) {
            Ok(ProbeMsg::Overflow(why)) => bail!("{why}"),
            Ok(ProbeMsg::Json(v)) => {
                // Auto-cancel any incoming server request (especially session/request_permission).
                if let (Some(method), Some(req_id)) =
                    (v["method"].as_str(), v.get("id").filter(|id| !id.is_null()))
                {
                    if !valid_rpc_id(req_id) {
                        bail!("ACP server sent an invalid or oversized request id");
                    }
                    let reply = if method == "session/request_permission" {
                        cancel_permission_reply(req_id)
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "error": { "code": -32601, "message": "unsupported in probe" },
                        })
                        .to_string()
                    };
                    let _ = writeln!(stdin, "{reply}").and_then(|()| stdin.flush());
                    continue;
                }
                if let Some(buf) = collect_text.as_deref_mut()
                    && v["method"] == "session/update"
                    && v["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
                    && let Some(chunk) = content_text(&v["params"]["update"]["content"])
                    && buf.len() < max_output
                {
                    let rem = max_output - buf.len();
                    let mut end = rem.min(chunk.len());
                    while end > 0 && !chunk.is_char_boundary(end) {
                        end -= 1;
                    }
                    buf.push_str(&chunk[..end]);
                }
                if v["method"].is_null() && v["id"].as_u64() == Some(wanted_id) {
                    return Ok(v);
                }
            }
            Err(flume::RecvTimeoutError::Disconnected) => {
                bail!("ACP process closed before answering")
            }
            Err(flume::RecvTimeoutError::Timeout) => bail!("ACP process did not answer in time"),
        }
    }
}

fn send_json(stdin: &mut std::process::ChildStdin, v: &Value) -> Result<()> {
    writeln!(stdin, "{v}").context("writing ACP request")?;
    stdin.flush().context("flushing ACP request")
}

fn reap_child(child: &mut skyelab_core::process::Owned, pid: u32) {
    #[cfg(unix)]
    crate::launch::kill_group(pid);
    let _ = child.kill();
    let _ = child.wait();
    #[cfg(not(unix))]
    let _ = pid;
}

/// Parses ACP `configOptions` (or legacy `models.availableModels`) into SkyeLab [`ModelInfo`] rows.
pub(crate) fn parse_models_from_session(result: &Value) -> Vec<ModelInfo> {
    let mut efforts = Vec::new();
    let mut default_effort = None;
    let mut current_model = None;
    let mut raw_models: Vec<(String, String)> = Vec::new();

    if let Some(opts) = result["configOptions"].as_array() {
        for opt in opts {
            if opt["category"] == "thought_level" && opt["type"] == "select" {
                default_effort = opt["currentValue"]
                    .as_str()
                    .filter(|v| valid_name(v))
                    .map(str::to_owned);
                efforts = flatten_select_options(&opt["options"])
                    .into_iter()
                    .take(MAX_EFFORTS)
                    .map(|(v, _)| v)
                    .collect();
            } else if opt["id"] == "model" && opt["type"] == "select" {
                current_model = opt["currentValue"].as_str().map(str::to_owned);
                raw_models = flatten_select_options(&opt["options"]);
            }
        }
    }

    if raw_models.is_empty()
        && let Some(avail) = result["models"]["availableModels"].as_array()
    {
        current_model = result["models"]["currentModelId"]
            .as_str()
            .map(str::to_owned);
        for m in avail.iter().take(MAX_MODELS) {
            if let Some(id) = m["modelId"].as_str().filter(|id| valid_name(id)) {
                let label = truncate_utf8(m["name"].as_str().unwrap_or(id), 256);
                raw_models.push((id.to_owned(), label));
            }
        }
    }

    raw_models
        .into_iter()
        .take(MAX_MODELS)
        .map(|(id, label)| ModelInfo {
            is_default: current_model.as_deref() == Some(id.as_str()),
            id,
            label,
            efforts: efforts.clone(),
            default_effort: default_effort.clone(),
        })
        .collect()
}

fn flatten_select_options(v: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(arr) = v.as_array() else {
        return out;
    };
    for entry in arr {
        if out.len() >= MAX_MODELS {
            break;
        }
        if let Some(val) = entry["value"].as_str().filter(|val| valid_name(val)) {
            let label = truncate_utf8(entry["name"].as_str().unwrap_or(val), 256);
            out.push((val.to_owned(), label));
        } else if let Some(group) = entry["options"].as_array() {
            for sub in group {
                if out.len() >= MAX_MODELS {
                    break;
                }
                if let Some(val) = sub["value"].as_str().filter(|val| valid_name(val)) {
                    let label = truncate_utf8(sub["name"].as_str().unwrap_or(val), 256);
                    out.push((val.to_owned(), label));
                }
            }
        }
    }
    out
}
