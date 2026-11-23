//! Codex: `codex app-server` over stdio, JSON-RPC one message per line (no `"jsonrpc"` field). One process
//! holds the thread: `thread/start` (or `thread/resume`), then a `turn/start` per prompt and `turn/interrupt`.
//! [`Codex`] is the client's state; it maps the server's notifications onto [`Event`], so the rest of
//! SkyeLab sees the same stream as from `claude`.

use crate::{Block, Decision, Event, Mode};
use anyhow::{Context as _, Result};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// `app-server` with every project layer off: `dir`, each ancestor and (for a worktree) its main repo are
/// marked untrusted, so a repo's `.codex/config.toml`, hooks and `.rules` never load. Overrides merge
/// with the user's `projects` table, which is why every ancestor is named, not just `dir`.
pub(crate) fn args(dir: &Path) -> Result<Vec<OsString>> {
    let mut roots: BTreeSet<&Path> = dir.ancestors().collect();
    let main = main_repo(dir);
    roots.extend(main.iter().flat_map(|m| m.ancestors()));
    let entries = roots
        .iter()
        .map(|p| {
            let p = p.to_str().context("folder path is not UTF-8")?;
            // A JSON string is a valid TOML basic string.
            Ok(format!("{}={{trust_level=\"untrusted\"}}", json!(p)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(vec![
        "app-server".into(),
        "-c".into(),
        format!("projects={{{}}}", entries.join(",")).into(),
    ])
}

/// The main checkout of the git worktree `dir` is in (from its `.git` file and `commondir`), if any.
fn main_repo(dir: &Path) -> Option<PathBuf> {
    let top = dir.ancestors().find(|a| a.join(".git").exists())?;
    let text = std::fs::read_to_string(top.join(".git")).ok()?; // a directory: a plain repo, no worktree
    let gitdir = top.join(text.strip_prefix("gitdir:")?.trim());
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let common = std::fs::canonicalize(gitdir.join(common.trim())).ok()?;
    common.parent().map(Path::to_owned)
}

/// The `config.mcp_servers` entry for SkyeLab's tools, from a grant file written for `claude --mcp-config`.
/// Sent over stdin, so the token never shows on a command line.
pub(crate) fn mcp_servers(grant: &Path) -> Result<Value> {
    let text =
        std::fs::read_to_string(grant).with_context(|| format!("reading {}", grant.display()))?;
    let v: Value = serde_json::from_str(&text).context("parsing the MCP grant")?;
    let s = &v["mcpServers"]["skyelab"];
    anyhow::ensure!(s["command"].is_string(), "the MCP grant names no command");
    Ok(json!({ "skyelab": { "command": s["command"], "args": s["args"], "env": s["env"] } }))
}

/// Client state for one app-server process.
pub(crate) struct Codex {
    next_id: u64,
    start_id: u64,
    thread_id: Option<String>,
    turn_id: Option<String>,
    /// The `turn/start` request still waiting for its answer.
    turn_req: Option<u64>,
    /// A turn was requested or runs: prompts queue behind it, like `claude`'s.
    busy: bool,
    interrupt_wanted: bool,
    queue: VecDeque<String>,
    last_text: Option<String>,
    /// Why codex refused to open the thread. Every later prompt fails with it instead of waiting forever.
    closed: Option<String>,
    /// Approval requests waiting for an answer: our id → codex's JSON-RPC request id.
    asks: BTreeMap<String, Value>,
    /// A mode picked after the thread opened, sent with the next `turn/start`.
    mode_change: Option<Mode>,
    /// The mode the `turn/start` in flight carries, put back if codex refuses that turn.
    mode_sent: Option<Mode>,
    /// Starts every ask id, new per process (see `claude::Book`).
    pub(crate) nonce: String,
}

/// Codex's `approvalPolicy` and `sandbox` for a mode. Codex has no plan mode, so Plan is a read-only sandbox.
pub(crate) fn policy(mode: Mode) -> (&'static str, &'static str) {
    match mode {
        // Edits inside the folder never ask under workspace-write, so Accept edits differs only for Claude.
        Mode::Default | Mode::AcceptEdits => ("untrusted", "workspace-write"),
        Mode::Plan => ("on-request", "read-only"),
        Mode::Bypass => ("never", "danger-full-access"),
    }
}

/// `turn/start`'s `sandboxPolicy` for the sandbox names `thread/start` takes.
fn sandbox_policy(sandbox: &str) -> Value {
    match sandbox {
        "read-only" => json!({ "type": "readOnly" }),
        "danger-full-access" => json!({ "type": "dangerFullAccess" }),
        _ => json!({ "type": "workspaceWrite" }),
    }
}

impl Codex {
    /// The state, and the lines that open the thread: `initialize`, then `thread/start` or `thread/resume`.
    /// Approvals and the sandbox follow `mode`; an approval request arrives as [`Event::Permission`].
    pub(crate) fn new(
        dir: &Path,
        resume: Option<&str>,
        mcp: Option<Value>,
        mode: Mode,
    ) -> (Self, Vec<String>) {
        let mut c = Codex {
            next_id: 0,
            start_id: 0,
            thread_id: None,
            turn_id: None,
            turn_req: None,
            busy: false,
            interrupt_wanted: false,
            queue: VecDeque::new(),
            last_text: None,
            closed: None,
            asks: BTreeMap::new(),
            mode_change: None,
            mode_sent: None,
            nonce: String::new(),
        };
        let init = c.request(
            "initialize",
            json!({ "clientInfo": {
                "name": "skyelab", "title": "SkyeLab", "version": env!("CARGO_PKG_VERSION"),
            }}),
        );
        let initialized = json!({ "method": "initialized", "params": {} }).to_string();
        let (approval, sandbox) = policy(mode);
        let mut params = json!({
            "cwd": dir,
            "approvalPolicy": approval,
            "sandbox": sandbox,
            "config": mcp.map(|m| json!({ "mcp_servers": m })),
        });
        let method = match resume {
            Some(id) => {
                params["threadId"] = json!(id);
                "thread/resume"
            }
            None => "thread/start",
        };
        c.start_id = c.next_id;
        let start = c.request(method, params);
        (c, vec![init, initialized, start])
    }

    fn request(&mut self, method: &str, params: Value) -> String {
        let id = self.next_id;
        self.next_id += 1;
        json!({ "id": id, "method": method, "params": params }).to_string()
    }

    pub(crate) fn prompt(&mut self, text: &str) -> Result<Vec<String>> {
        if let Some(why) = &self.closed {
            anyhow::bail!("codex couldn't open this thread ({why}); start a new thread");
        }
        self.queue.push_back(text.to_owned());
        Ok(self.pump())
    }

    /// Starts the next queued turn once the thread is open and no turn runs.
    fn pump(&mut self) -> Vec<String> {
        let (Some(thread), false) = (self.thread_id.clone(), self.busy) else {
            return Vec::new();
        };
        let Some(text) = self.queue.pop_front() else {
            return Vec::new();
        };
        self.busy = true;
        self.turn_req = Some(self.next_id);
        let mut params = json!({ "threadId": thread, "input": [{ "type": "text", "text": text }] });
        self.mode_sent = self.mode_change;
        if let Some(mode) = self.mode_change.take() {
            // Overrides stick for this turn and the ones after it.
            let (approval, sandbox) = policy(mode);
            params["approvalPolicy"] = json!(approval);
            params["sandboxPolicy"] = sandbox_policy(sandbox);
        }
        vec![self.request("turn/start", params)]
    }

    pub(crate) fn set_mode(&mut self, mode: Mode) {
        self.mode_change = Some(mode);
    }

    /// The reply to approval request `id`, or `None` when it isn't waiting. It stays open until [`Self::forget`].
    pub(crate) fn answer(&mut self, id: &str, decision: Decision) -> Option<String> {
        let rpc = self.asks.get(id)?;
        let decision = match decision {
            Decision::AllowOnce => "accept",
            Decision::AllowThread => "acceptForSession",
            Decision::Deny | Decision::Withdrawn => "decline",
        };
        Some(json!({ "id": rpc, "result": { "decision": decision } }).to_string())
    }

    pub(crate) fn forget(&mut self, id: &str) {
        self.asks.remove(id);
    }

    /// Every open request, withdrawn: its turn or its process is over.
    pub(crate) fn withdraw_all(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.asks)
            .into_keys()
            .map(|id| Event::PermissionResolved {
                id,
                decision: Decision::Withdrawn,
            })
            .collect()
    }

    /// A server request: an approval waits for the user; anything else is answered now.
    fn on_request(
        &mut self,
        method: &str,
        rpc: Value,
        params: &Value,
    ) -> (Vec<Event>, Vec<String>) {
        let (tool, keys): (_, &[&str]) = match method {
            "item/commandExecution/requestApproval" => ("shell", &["command", "cwd", "reason"]),
            "item/fileChange/requestApproval" => ("apply_patch", &["grantRoot", "reason"]),
            _ => return (Vec::new(), vec![answer(method, rpc)]),
        };
        // Only the fields codex sent: an absent `grantRoot` must not show as null.
        let input: serde_json::Map<String, Value> = keys
            .iter()
            .filter(|k| !params[**k].is_null())
            .map(|k| ((*k).to_owned(), params[*k].clone()))
            .collect();
        let input = Value::Object(input);
        let id = format!("{}codex-{}", self.nonce, rpc);
        self.asks.insert(id.clone(), rpc);
        let event = Event::Permission {
            id,
            tool: tool.to_owned(),
            input,
        };
        (vec![event], Vec::new())
    }

    /// Stops the running turn; before its id is known, as soon as it is.
    pub(crate) fn interrupt(&mut self) -> Vec<String> {
        match (self.thread_id.clone(), self.turn_id.clone()) {
            (Some(thread), Some(turn)) => {
                self.interrupt_wanted = false;
                vec![self.request(
                    "turn/interrupt",
                    json!({ "threadId": thread, "turnId": turn }),
                )]
            }
            _ => {
                // A prompt waiting for the thread to open is the turn the user means to stop.
                self.interrupt_wanted = self.busy || !self.queue.is_empty();
                Vec::new()
            }
        }
    }

    fn turn_known(&mut self, id: Option<&str>) -> Vec<String> {
        if self.turn_id.is_none() {
            self.turn_id = id.map(str::to_owned);
        }
        if self.interrupt_wanted {
            self.interrupt()
        } else {
            Vec::new()
        }
    }

    /// One line from codex: the events it means, and the lines to write back.
    pub(crate) fn on_line(&mut self, v: Value) -> (Vec<Event>, Vec<String>) {
        let method = v["method"].as_str().map(str::to_owned);
        match (method, v.get("id").filter(|id| !id.is_null()).cloned()) {
            (Some(method), Some(id)) => self.on_request(&method, id, &v["params"]),
            (None, Some(id)) => self.on_response(id.as_u64(), &v),
            (Some(method), None) => self.on_notification(&method, v),
            (None, None) => (vec![Event::Unknown(v)], Vec::new()),
        }
    }

    fn on_response(&mut self, id: Option<u64>, v: &Value) -> (Vec<Event>, Vec<String>) {
        let error = v["error"]["message"].as_str().map(str::to_owned);
        if id == Some(self.start_id) {
            return match (error, v["result"]["thread"]["id"].as_str()) {
                (None, Some(thread)) => {
                    self.thread_id = Some(thread.to_owned());
                    let init = Event::Init {
                        session_id: thread.to_owned(),
                        model: v["result"]["model"].as_str().unwrap_or_default().to_owned(),
                    };
                    (vec![init], self.pump())
                }
                (error, _) => {
                    let why = error.unwrap_or_else(|| "codex opened no thread".into());
                    // The Result below answers every prompt that was waiting for the thread.
                    let waiting = self.queue.len().max(1); // the launch's own prompt, at least
                    self.queue.clear();
                    self.closed = Some(why.clone());
                    let mut result = failed(why);
                    if let Event::Result { prompts, .. } = &mut result {
                        *prompts = u32::try_from(waiting).unwrap_or(u32::MAX);
                    }
                    (vec![result], Vec::new())
                }
            };
        }
        if id.is_none() || id != self.turn_req {
            return (Vec::new(), Vec::new());
        }
        self.turn_req = None;
        match error {
            None => {
                let turn = v["result"]["turn"]["id"].as_str();
                (Vec::new(), self.turn_known(turn))
            }
            Some(e) => {
                self.end_turn();
                // Refused, so its mode change never applied: the next turn carries it.
                self.mode_change = self.mode_change.or(self.mode_sent.take());
                (vec![failed(e)], self.pump())
            }
        }
    }

    fn end_turn(&mut self) {
        (self.busy, self.turn_id, self.interrupt_wanted) = (false, None, false);
    }

    fn on_notification(&mut self, method: &str, v: Value) -> (Vec<Event>, Vec<String>) {
        let p = &v["params"];
        match method {
            "turn/started" => (Vec::new(), self.turn_known(p["turn"]["id"].as_str())),
            "turn/completed" => {
                let turn = &p["turn"];
                let ok = turn["status"] == "completed";
                let text = self.last_text.take();
                let result = Event::Result {
                    is_error: !ok,
                    duration_ms: turn["durationMs"].as_u64().unwrap_or_default(),
                    total_cost_usd: 0.0,
                    prompts: 1,
                    result: turn["error"]["message"]
                        .as_str()
                        .map(str::to_owned)
                        .or(if ok {
                            text
                        } else {
                            turn["status"].as_str().map(str::to_owned)
                        }),
                };
                self.end_turn();
                let mut events = self.withdraw_all();
                events.push(result);
                (events, self.pump())
            }
            "serverRequest/resolved" => {
                let id = format!("{}codex-{}", self.nonce, p["requestId"]);
                let events = self.asks.remove(&id).map(|_| Event::PermissionResolved {
                    id,
                    decision: Decision::Withdrawn,
                });
                (events.into_iter().collect(), Vec::new())
            }
            "item/started" => (started(&p["item"]).into_iter().collect(), Vec::new()),
            "item/completed" => {
                let item = &p["item"];
                if item["type"] == "agentMessage" {
                    self.last_text = item["text"].as_str().map(str::to_owned);
                }
                (completed(item, &v).into_iter().collect(), Vec::new())
            }
            // Streaming deltas and status chatter: the completed items carry what matters.
            m if NOISE.iter().any(|n| m.starts_with(n)) => (Vec::new(), Vec::new()),
            _ => (vec![Event::Unknown(v)], Vec::new()),
        }
    }
}

const NOISE: &[&str] = &[
    "item/",
    "thread/",
    "turn/",
    "account/",
    "mcpServer/",
    "remoteControl/",
    "serverRequest/",
];

/// A server request SkyeLab has no UI for (user input, MCP elicitation, old-style approvals): unsupported, so codex
/// treats it as refused.
fn answer(method: &str, id: Value) -> String {
    json!({ "id": id, "error": { "code": -32601, "message": format!("SkyeLab doesn't handle {method}") } })
        .to_string()
}

fn failed(message: String) -> Event {
    Event::Result {
        is_error: true,
        duration_ms: 0,
        total_cost_usd: 0.0,
        result: Some(message),
        prompts: 1,
    }
}

fn started(item: &Value) -> Option<Event> {
    let (name, input) = match item["type"].as_str()? {
        "commandExecution" => ("shell".to_owned(), json!({ "command": item["command"] })),
        "fileChange" => {
            let files: Vec<_> = item["changes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| c["path"].clone())
                .collect();
            ("apply_patch".to_owned(), json!({ "files": files }))
        }
        "mcpToolCall" => (
            format!(
                "mcp__{}__{}",
                item["server"].as_str()?,
                item["tool"].as_str()?
            ),
            item["arguments"].clone(),
        ),
        "webSearch" => ("web_search".to_owned(), json!({ "query": item["query"] })),
        _ => return None,
    };
    Some(Event::Assistant {
        content: vec![Block::ToolUse {
            id: item["id"].as_str()?.to_owned(),
            name,
            input,
        }],
    })
}

fn completed(item: &Value, raw: &Value) -> Option<Event> {
    let status = item["status"].as_str().unwrap_or("completed");
    let (content, is_error) = match item["type"].as_str()? {
        "agentMessage" => {
            let text = item["text"].as_str().filter(|t| !t.is_empty())?;
            return Some(Event::Assistant {
                content: vec![Block::Text {
                    text: text.to_owned(),
                }],
            });
        }
        // Already shown: the prompt, and hidden reasoning.
        "userMessage" | "reasoning" | "contextCompaction" => return None,
        "commandExecution" => (
            item["aggregatedOutput"].clone(),
            status != "completed" || item["exitCode"].as_i64().is_some_and(|c| c != 0),
        ),
        "fileChange" => (json!(status), status != "completed"),
        "mcpToolCall" => match item["error"]["message"].as_str() {
            Some(e) => (json!(e), true),
            None => (item["result"].clone(), status == "failed"),
        },
        "webSearch" => (json!(""), false),
        _ => return Some(Event::Unknown(raw.clone())),
    };
    Some(Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: item["id"].as_str()?.to_owned(),
            content: if content.is_null() {
                json!("")
            } else {
                content
            },
            is_error,
        }],
    })
}

#[cfg(test)]
#[path = "codex_tests.rs"]
mod tests;
