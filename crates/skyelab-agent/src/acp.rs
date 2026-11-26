//! Reusable Agent Client Protocol (ACP) stdio JSON-RPC 2.0 client state machine.
//!
//! Provider-agnostic: any ACP CLI (Antigravity, OpenCode, Kiro, Grok, Gemini CLI) supplies an
//! [`AcpConfig`] plus its own command/env setup, while this module handles the full JSON-RPC 2.0
//! lifecycle (`initialize`, `session/new`, `session/resume`, `session/load`, `session/prompt`,
//! `session/cancel`, `session/set_config_option`, `session/set_mode`, `session/update`, and
//! `session/request_permission`).

use crate::acp_asks::Asks;
pub(crate) use crate::acp_rpc::*;
use crate::acp_updates::{ToolCallTracker, TurnBuffers, handle_update};
use crate::{Attachment, Decision, Event, Mode, attach};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

/// Hard cap on [`Event`]s produced from a single ACP stdout line.
pub(crate) const MAX_EVENTS_PER_LINE: usize = 16;

/// Provider-specific parameters for an ACP session.
#[derive(Clone)]
pub(crate) struct AcpConfig {
    pub(crate) client_name: &'static str,
    pub(crate) client_version: &'static str,
    pub(crate) provider_label: &'static str,
}

#[derive(Clone, Default)]
pub(crate) struct Pick {
    pub(crate) model: Option<String>,
    pub(crate) effort: Option<String>,
}

/// Client state for one ACP stdio process.
pub(crate) struct Acp {
    cfg: AcpConfig,
    cwd: PathBuf,
    mcp_servers: Vec<Value>,
    resume: Option<String>,
    next_id: u64,
    init_req: Option<u64>,
    open_req: Option<u64>,
    prompt_req: Option<u64>,
    session_id: Option<String>,
    /// Prompt payloads queued while the session opens or another turn runs.
    queue: VecDeque<Value>,
    busy: bool,
    interrupt_wanted: bool,
    /// Why opening the session failed; every subsequent prompt fails fast with this message.
    closed: Option<String>,
    pub(crate) pick: Pick,
    thought_config_id: Option<String>,
    available_modes: Vec<String>,
    pending_mode: Option<Mode>,
    tools: ToolCallTracker,
    buffers: TurnBuffers,
    asks: Asks,
}

impl Acp {
    /// Builds the ACP state and its opening `initialize` request line.
    pub(crate) fn new(
        cfg: AcpConfig,
        cwd: &Path,
        resume: Option<&str>,
        mcp_servers: Vec<Value>,
        mode: Mode,
        pick: Pick,
        nonce: String,
    ) -> (Self, Vec<String>) {
        let mut a = Self {
            cfg,
            cwd: cwd.to_path_buf(),
            mcp_servers,
            resume: resume.map(str::to_owned),
            next_id: 1,
            init_req: None,
            open_req: None,
            prompt_req: None,
            session_id: None,
            queue: VecDeque::new(),
            busy: false,
            interrupt_wanted: false,
            closed: None,
            pick,
            thought_config_id: None,
            available_modes: Vec::new(),
            pending_mode: (mode != Mode::Default).then_some(mode),
            tools: ToolCallTracker::with_nonce(&nonce),
            buffers: TurnBuffers::default(),
            asks: Asks::new(nonce),
        };
        let init_id = a.alloc_id();
        a.init_req = Some(init_id);
        let init = a.build_request(init_id, "initialize", initialize_params(&a.cfg));
        (a, vec![init])
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn build_request(&self, id: u64, method: &str, params: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
        .to_string()
    }

    fn request(&mut self, method: &str, params: Value) -> (u64, String) {
        let id = self.alloc_id();
        let line = self.build_request(id, method, params);
        (id, line)
    }

    /// Queues or immediately sends a user prompt as an ACP `session/prompt` request.
    pub(crate) fn prompt(&mut self, text: &str, attachments: &[Attachment]) -> Result<Vec<String>> {
        if let Some(why) = &self.closed {
            bail!("{why}");
        }
        let blocks = attach::acp_prompt_blocks(text, attachments)?;
        self.queue.push_back(blocks);
        Ok(self.pump())
    }

    /// Stops the running turn via `session/cancel` (or remembers it if the session is still opening).
    pub(crate) fn interrupt(&mut self) -> Vec<String> {
        if let (Some(session_id), true) = (self.session_id.clone(), self.busy) {
            self.interrupt_wanted = true;
            vec![
                json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": { "sessionId": session_id },
                })
                .to_string(),
            ]
        } else {
            self.interrupt_wanted = self.busy || !self.queue.is_empty();
            Vec::new()
        }
    }

    /// Updates the permission mode, sending `session/set_mode` if supported by the session.
    pub(crate) fn set_mode(&mut self, mode: Mode) -> Vec<String> {
        self.pending_mode = Some(mode);
        self.apply_mode()
    }

    /// Updates model and/or effort, sending `session/set_config_option` if the session is open.
    pub(crate) fn set_model(&mut self, model: Option<&str>, effort: Option<&str>) -> Vec<String> {
        self.pick = Pick {
            model: model.map(str::to_owned),
            effort: effort.map(str::to_owned),
        };
        if self.session_id.is_none() {
            return Vec::new();
        }
        self.apply_pick()
    }

    pub(crate) fn answer(&self, id: &str, decision: Decision) -> Option<String> {
        self.asks.answer(id, decision)
    }

    pub(crate) fn answer_questions(&self, id: &str, answers: &[Vec<String>]) -> Option<String> {
        self.asks.answer_questions(id, answers)
    }

    pub(crate) fn forget(&mut self, id: &str) {
        self.asks.forget(id);
    }

    pub(crate) fn withdraw_all(&mut self) -> Vec<Event> {
        self.asks.withdraw_all()
    }

    /// Processes one parsed JSON-RPC 2.0 line from stdout, returning events and stdin replies.
    pub(crate) fn on_line(&mut self, v: Value) -> (Vec<Event>, Vec<String>) {
        let method = v["method"].as_str().map(str::to_owned);
        let id = v.get("id").filter(|id| !id.is_null()).cloned();
        let (mut events, replies) = match (method, id) {
            (Some(m), Some(id)) => self.on_request(&m, id, &v["params"]),
            (None, Some(id)) => self.on_response(id.as_u64(), &v),
            (Some(m), None) => self.on_notification(&m, v),
            (None, None) => (vec![Event::Unknown(v)], Vec::new()),
        };
        if events.len() > MAX_EVENTS_PER_LINE {
            events.truncate(MAX_EVENTS_PER_LINE);
        }
        (events, replies)
    }

    fn on_request(&mut self, method: &str, id: Value, params: &Value) -> (Vec<Event>, Vec<String>) {
        if !valid_rpc_id(&id) {
            return (Vec::new(), Vec::new());
        }
        match method {
            "session/request_permission" => self.asks.on_request(id, params),
            _ => (
                Vec::new(),
                vec![
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("unsupported method {method}") },
                    })
                    .to_string(),
                ],
            ),
        }
    }

    fn on_notification(&mut self, method: &str, v: Value) -> (Vec<Event>, Vec<String>) {
        if method != "session/update" {
            return (vec![Event::Unknown(v)], Vec::new());
        }
        let update = &v["params"]["update"];
        if update["sessionUpdate"] == "config_option_update" {
            self.capture_config_options(&update["configOptions"]);
            return (Vec::new(), Vec::new());
        }
        if matches!(
            update["sessionUpdate"].as_str(),
            Some("current_mode_update" | "available_commands_update" | "session_info_update")
        ) {
            return (Vec::new(), Vec::new());
        }
        let events = handle_update(update, &mut self.tools, &mut self.buffers);
        (events, Vec::new())
    }

    fn on_response(&mut self, id: Option<u64>, v: &Value) -> (Vec<Event>, Vec<String>) {
        let Some(id) = id else {
            return (Vec::new(), Vec::new());
        };
        if Some(id) == self.init_req {
            self.init_req = None;
            return self.on_initialize(v);
        }
        if Some(id) == self.open_req {
            self.open_req = None;
            return self.on_session_opened(v);
        }
        if Some(id) == self.prompt_req {
            self.prompt_req = None;
            return self.on_prompt_done(v);
        }
        (Vec::new(), Vec::new())
    }

    fn on_initialize(&mut self, v: &Value) -> (Vec<Event>, Vec<String>) {
        if let Some(err) = rpc_error_message(v) {
            return self.fail_open(err, false);
        }
        let caps = &v["result"]["agentCapabilities"];
        let supports_resume = caps["sessionCapabilities"]["resume"].is_object()
            || caps["session_capabilities"]["resume"].is_object();
        let supports_load = caps["loadSession"].as_bool().unwrap_or(false)
            || caps["load_session"].as_bool().unwrap_or(false);

        let (method, params) = match &self.resume {
            Some(session_id) if supports_resume => (
                "session/resume",
                json!({
                    "sessionId": session_id,
                    "cwd": self.cwd,
                    "mcpServers": self.mcp_servers,
                }),
            ),
            Some(session_id) if supports_load => (
                "session/load",
                json!({
                    "sessionId": session_id,
                    "cwd": self.cwd,
                    "mcpServers": self.mcp_servers,
                }),
            ),
            Some(_) => {
                return self.fail_open(
                    format!(
                        "{} does not support resuming sessions",
                        self.cfg.provider_label
                    ),
                    false,
                );
            }
            None => (
                "session/new",
                json!({
                    "cwd": self.cwd,
                    "mcpServers": self.mcp_servers,
                }),
            ),
        };
        let (req_id, line) = self.request(method, params);
        self.open_req = Some(req_id);
        (Vec::new(), vec![line])
    }

    fn on_session_opened(&mut self, v: &Value) -> (Vec<Event>, Vec<String>) {
        if let Some(msg) = rpc_error_message(v) {
            let code = v["error"]["code"].as_i64().unwrap_or(0);
            if is_auth_required_error(code, &msg) {
                return self.fail_open(
                    format!(
                        "{} is not signed in. Log in from Settings → Accounts.",
                        self.cfg.provider_label
                    ),
                    false,
                );
            }
            let gone = self.resume.is_some() && is_session_gone_error(code, &msg);
            return self.fail_open(msg, gone);
        }
        let result = &v["result"];
        let session_id = result["sessionId"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| self.resume.clone())
            .unwrap_or_default();
        if session_id.is_empty() {
            return self.fail_open(
                format!("{} returned no sessionId", self.cfg.provider_label),
                false,
            );
        }
        self.session_id = Some(session_id.clone());
        self.capture_config_options(&result["configOptions"]);
        self.capture_modes(&result["modes"]);
        let current_model = current_model_from_result(result)
            .or_else(|| self.pick.model.clone())
            .unwrap_or_default();

        let mut replies = self.apply_pick();
        replies.extend(self.apply_mode());
        replies.extend(self.pump());
        (
            vec![Event::Init {
                session_id,
                model: current_model,
            }],
            replies,
        )
    }

    fn on_prompt_done(&mut self, v: &Value) -> (Vec<Event>, Vec<String>) {
        let mut events = Vec::new();
        if let Some(a) = self.buffers.flush() {
            events.push(a);
        }
        events.extend(self.asks.withdraw_all());
        self.tools.clear();
        let last_text = self.buffers.last_text.take();
        self.buffers.clear();
        let was_interrupted = std::mem::take(&mut self.interrupt_wanted);
        self.busy = false;

        let result = if let Some(err) = rpc_error_message(v) {
            Event::Result {
                is_error: true,
                duration_ms: 0,
                total_cost_usd: 0.0,
                result: Some(err),
                prompts: 1,
                usage: None,
                turn_cost_usd: None,
            }
        } else if v["result"]["stopReason"] == "cancelled" || was_interrupted {
            Event::Result {
                is_error: true,
                duration_ms: 0,
                total_cost_usd: 0.0,
                result: Some("interrupted".into()),
                prompts: 1,
                usage: None,
                turn_cost_usd: None,
            }
        } else {
            Event::Result {
                is_error: false,
                duration_ms: 0,
                total_cost_usd: 0.0,
                result: last_text,
                prompts: 1,
                usage: None,
                turn_cost_usd: None,
            }
        };
        events.push(result);
        let replies = self.pump();
        (events, replies)
    }

    fn fail_open(&mut self, why: String, gone: bool) -> (Vec<Event>, Vec<String>) {
        let waiting = self.queue.len().max(1);
        self.queue.clear();
        self.busy = false;
        self.interrupt_wanted = false;
        self.closed = Some(why.clone());
        let result = Event::Result {
            is_error: true,
            duration_ms: 0,
            total_cost_usd: 0.0,
            result: Some(why),
            prompts: u32::try_from(waiting).unwrap_or(u32::MAX),
            usage: None,
            turn_cost_usd: None,
        };
        let events = if gone {
            vec![Event::SessionGone, result]
        } else {
            vec![result]
        };
        (events, Vec::new())
    }

    fn pump(&mut self) -> Vec<String> {
        let (Some(session_id), false) = (self.session_id.clone(), self.busy) else {
            return Vec::new();
        };
        let Some(prompt) = self.queue.pop_front() else {
            return Vec::new();
        };
        self.busy = true;
        let (id, line) = self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": prompt,
            }),
        );
        self.prompt_req = Some(id);
        let mut out = vec![line];
        if self.interrupt_wanted {
            out.push(
                json!({
                    "jsonrpc": "2.0",
                    "method": "session/cancel",
                    "params": { "sessionId": session_id },
                })
                .to_string(),
            );
        }
        out
    }

    fn apply_pick(&mut self) -> Vec<String> {
        let Some(session_id) = self.session_id.clone() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(model) = self.pick.model.clone() {
            let (_, line) = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": "model",
                    "value": model,
                }),
            );
            out.push(line);
        }
        if let (Some(effort), Some(cfg_id)) =
            (self.pick.effort.clone(), self.thought_config_id.clone())
        {
            let (_, line) = self.request(
                "session/set_config_option",
                json!({
                    "sessionId": session_id,
                    "configId": cfg_id,
                    "value": effort,
                }),
            );
            out.push(line);
        }
        out
    }

    fn apply_mode(&mut self) -> Vec<String> {
        let (Some(session_id), Some(mode)) = (self.session_id.clone(), self.pending_mode) else {
            return Vec::new();
        };
        let target = acp_mode_id(mode);
        if !self.available_modes.iter().any(|m| m == target) {
            return Vec::new();
        }
        self.pending_mode = None;
        let (_, line) = self.request(
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": target,
            }),
        );
        vec![line]
    }

    fn capture_config_options(&mut self, options: &Value) {
        if let Some(arr) = options.as_array() {
            for opt in arr {
                if opt["category"] == "thought_level"
                    && let Some(id) = opt["id"].as_str()
                {
                    self.thought_config_id = Some(id.to_owned());
                }
            }
        }
    }

    fn capture_modes(&mut self, modes: &Value) {
        if let Some(arr) = modes["availableModes"].as_array() {
            self.available_modes = arr
                .iter()
                .filter_map(|m| m["id"].as_str().map(str::to_owned))
                .take(32)
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_init_response_and_oversized_rpc_ids_are_ignored() {
        let (mut acp, _) = Acp::new(
            crate::antigravity::config(),
            Path::new("/tmp"),
            None,
            Vec::new(),
            Mode::Default,
            Pick::default(),
            "n-".into(),
        );
        let (_, replies) = acp.on_line(json!({ "jsonrpc": "2.0", "id": 1, "result": {} }));
        assert_eq!(replies.len(), 1, "first init triggers session/new");

        // A duplicate id=1 response (even an error) must not re-initialize or close the session.
        let (events, replies) = acp.on_line(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": { "code": -32603, "message": "duplicate" }
        }));
        assert!(events.is_empty() && replies.is_empty());

        // Oversized or non-scalar request IDs are dropped without allocating or replying.
        assert!(!valid_rpc_id(&json!({ "nested": 1 })));
        assert!(!valid_rpc_id(&json!("x".repeat(300))));
        assert!(valid_rpc_id(&json!(42)));
        assert!(valid_rpc_id(&json!("req-1")));
        let (events, replies) = acp.on_line(json!({
            "jsonrpc": "2.0",
            "id": "x".repeat(300),
            "method": "session/request_permission",
            "params": {}
        }));
        assert!(events.is_empty() && replies.is_empty());

        // An error object without an integer code still fails session open cleanly.
        let (events, _) = acp.on_line(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "error": { "message": "boom" }
        }));
        assert!(matches!(
            events.as_slice(),
            [Event::Result { is_error: true, .. }]
        ));
    }
}
