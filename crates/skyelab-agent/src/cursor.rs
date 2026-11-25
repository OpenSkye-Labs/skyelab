//! Cursor CLI (`cursor-agent` / `agent`) protocol driver: bounded stream-json parsing, tool normalization,
//! binary resolution (`cursor-agent` with `agent` fallback), and argument construction.

use crate::{Block, DeltaKind, Event, Mode, Usage};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Most content blocks kept from one assistant JSON line.
const MAX_BLOCKS_PER_LINE: usize = 64;
/// Most bytes buffered for in-flight thinking or streaming text deltas.
const MAX_ACCUM_BYTES: usize = 8 << 20;
/// Most tracked in-flight tool call ids.
const MAX_OPEN_TOOLS: usize = 1024;
/// Most events emitted for one Cursor turn before stopping with an error result.
const MAX_SESSION_EVENTS: usize = 100_000;

/// Resolves the Cursor CLI binary from `PATH`. Bare `"cursor-agent"` falls back to `"agent"`;
/// explicit absolute paths and other names resolve strictly without fallback.
pub fn resolve_cursor_bin(bin: &Path) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = skyelab_core::env::absolute_entries(&path);
    resolve_cursor_bin_in(bin, &dirs)
}

/// Resolves `bin` in `path_dirs`. Bare `"cursor-agent"` tries `"cursor-agent"` first, then `"agent"`.
pub fn resolve_cursor_bin_in(bin: &Path, path_dirs: &[PathBuf]) -> Result<PathBuf> {
    if bin.is_absolute() {
        return Ok(bin.to_owned());
    }
    if bin.components().count() != 1 {
        bail!("{} must be an absolute path or a bare name", bin.display());
    }
    if bin == Path::new("cursor-agent") {
        if let Ok(found) = crate::launch::resolve_bin_in(bin, path_dirs) {
            return Ok(found);
        }
        if let Ok(found) = crate::launch::resolve_bin_in(Path::new("agent"), path_dirs) {
            return Ok(found);
        }
        bail!("cursor-agent (or agent) not found in PATH");
    }
    crate::launch::resolve_bin_in(bin, path_dirs)
}

/// Builds the non-interactive streaming arguments for one Cursor turn. Prompt text is written on stdin.
pub(crate) fn args(
    cmd: &mut Command,
    dir: &Path,
    resume: Option<&str>,
    model: Option<&str>,
    mode: Mode,
) {
    cmd.args([
        "-p",
        "--output-format",
        "stream-json",
        "--stream-partial-output",
    ]);
    cmd.arg("--workspace").arg(dir);
    if let Some(m) = model {
        cmd.args(["--model", m]);
    }
    match mode {
        Mode::Plan => {
            cmd.args(["--mode", "plan"]);
        }
        Mode::Bypass => {
            cmd.arg("--force");
        }
        // Default and AcceptEdits never silently escalate to --force.
        Mode::Default | Mode::AcceptEdits => {}
    }
    if let Some(id) = resume {
        cmd.args(["--resume", id]);
    }
}

/// Bounded per-turn state for translating Cursor's NDJSON stream into [`Event`]s.
pub(crate) struct CursorState {
    session_id: Option<String>,
    model: Option<String>,
    resuming: bool,
    pub(crate) interrupted: bool,
    init_emitted: bool,
    result_emitted: bool,
    capped: bool,
    emitted_events: usize,
    thinking_buf: String,
    text_buf: String,
    open_tools: VecDeque<String>,
    open_tool_set: HashSet<String>,
}

impl CursorState {
    pub(crate) fn new(resuming: bool, model: Option<&str>) -> Self {
        Self {
            session_id: None,
            model: model.map(str::to_owned),
            resuming,
            interrupted: false,
            init_emitted: false,
            result_emitted: false,
            capped: false,
            emitted_events: 0,
            thinking_buf: String::new(),
            text_buf: String::new(),
            open_tools: VecDeque::new(),
            open_tool_set: HashSet::new(),
        }
    }

    /// Translates one stdout line from `cursor-agent` into zero or more [`Event`]s.
    pub(crate) fn parse(&mut self, line: &str) -> Vec<Event> {
        if self.capped {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            let mut out = Vec::new();
            self.emit(&mut out, Event::Unknown(Value::String(line.to_owned())));
            return out;
        };
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or_default() {
            "system" => self.on_system(&v, &mut out),
            "user" => {}
            "thinking" => self.on_thinking(&v, &mut out),
            "tool_call" => self.on_tool_call(&v, &mut out),
            "assistant" => self.on_assistant(&v, &mut out),
            "result" => self.on_result(&v, &mut out),
            _ => {
                self.emit(&mut out, Event::Unknown(v));
            }
        }
        out
    }

    /// Translates one truncated oversized stdout line through the session event cap.
    pub(crate) fn on_oversized(&mut self, line: String) -> Vec<Event> {
        if self.capped {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.emit(&mut out, Event::Unknown(Value::String(line)));
        out
    }

    /// Called when stdout reaches EOF: flushes any buffered partial blocks and emits an interrupted result if needed.
    pub(crate) fn on_eof(&mut self) -> Vec<Event> {
        if self.capped || self.result_emitted {
            return Vec::new();
        }
        let mut out = Vec::new();
        self.flush_thinking(&mut out);
        self.flush_text(&mut out);
        if self.interrupted {
            self.result_emitted = true;
            self.emit(
                &mut out,
                Event::Result {
                    is_error: true,
                    duration_ms: 0,
                    total_cost_usd: 0.0,
                    result: Some("interrupted".into()),
                    prompts: 1,
                    usage: None,
                    turn_cost_usd: None,
                },
            );
        }
        out
    }

    fn emit(&mut self, out: &mut Vec<Event>, event: Event) -> bool {
        if self.capped {
            return false;
        }
        if self.emitted_events >= MAX_SESSION_EVENTS {
            self.capped = true;
            self.result_emitted = true;
            out.push(Event::Result {
                is_error: true,
                duration_ms: 0,
                total_cost_usd: 0.0,
                result: Some("Cursor output exceeded event cap".into()),
                prompts: 1,
                usage: None,
                turn_cost_usd: None,
            });
            return false;
        }
        self.emitted_events += 1;
        out.push(event);
        true
    }

    fn on_system(&mut self, v: &Value, out: &mut Vec<Event>) {
        if v["subtype"].as_str() != Some("init") {
            return;
        }
        if let Some(id) = v["session_id"].as_str().filter(|s| !s.is_empty()) {
            self.session_id = Some(id.to_owned());
        }
        if let Some(m) = v["model"].as_str().filter(|s| !s.is_empty()) {
            self.model = Some(m.to_owned());
        }
        if !self.init_emitted {
            self.init_emitted = true;
            let session_id = self.session_id.clone().unwrap_or_default();
            let model = self.model.clone().unwrap_or_default();
            self.emit(out, Event::Init { session_id, model });
        }
    }

    fn on_thinking(&mut self, v: &Value, out: &mut Vec<Event>) {
        match v["subtype"].as_str().unwrap_or_default() {
            "delta" => {
                if let Some(text) = v["text"].as_str()
                    && !text.is_empty()
                {
                    push_capped(&mut self.thinking_buf, text, MAX_ACCUM_BYTES);
                    self.emit(
                        out,
                        Event::Delta {
                            kind: DeltaKind::Thinking,
                            text: text.to_owned(),
                        },
                    );
                }
            }
            "completed" => {
                if let Some(text) = v["text"].as_str()
                    && !text.is_empty()
                    && self.thinking_buf.is_empty()
                {
                    push_capped(&mut self.thinking_buf, text, MAX_ACCUM_BYTES);
                }
                self.flush_thinking(out);
            }
            _ => {}
        }
    }

    fn flush_thinking(&mut self, out: &mut Vec<Event>) {
        if !self.thinking_buf.is_empty() {
            let thinking = std::mem::take(&mut self.thinking_buf);
            self.emit(
                out,
                Event::Assistant {
                    content: vec![Block::Thinking { thinking }],
                },
            );
        }
    }

    fn flush_text(&mut self, out: &mut Vec<Event>) {
        if !self.text_buf.is_empty() {
            let text = std::mem::take(&mut self.text_buf);
            self.emit(
                out,
                Event::Assistant {
                    content: vec![Block::Text { text }],
                },
            );
        }
    }

    fn on_tool_call(&mut self, v: &Value, out: &mut Vec<Event>) {
        self.flush_thinking(out);
        self.flush_text(out);
        let subtype = v["subtype"].as_str().unwrap_or_default();
        let id = v["call_id"]
            .as_str()
            .or_else(|| v["tool_call_id"].as_str())
            .or_else(|| v["id"].as_str())
            .unwrap_or_default()
            .to_owned();
        match subtype {
            "started" => {
                let (name, input) = extract_tool_start(v);
                self.track_tool(&id);
                self.emit(
                    out,
                    Event::Assistant {
                        content: vec![Block::ToolUse { id, name, input }],
                    },
                );
            }
            "completed" | "failed" => {
                self.untrack_tool(&id);
                let (content, is_error) = extract_tool_result(v, subtype == "failed");
                self.emit(
                    out,
                    Event::User {
                        content: vec![Block::ToolResult {
                            tool_use_id: id,
                            content,
                            is_error,
                        }],
                    },
                );
            }
            _ => {}
        }
    }

    fn track_tool(&mut self, id: &str) {
        if id.is_empty() || self.open_tool_set.contains(id) {
            return;
        }
        if self.open_tools.len() >= MAX_OPEN_TOOLS
            && let Some(oldest) = self.open_tools.pop_front()
        {
            self.open_tool_set.remove(&oldest);
        }
        self.open_tools.push_back(id.to_owned());
        self.open_tool_set.insert(id.to_owned());
    }

    fn untrack_tool(&mut self, id: &str) {
        if self.open_tool_set.remove(id) {
            self.open_tools.retain(|existing| existing != id);
        }
    }

    fn on_assistant(&mut self, v: &Value, out: &mut Vec<Event>) {
        self.flush_thinking(out);
        let Some(raw_blocks) = v["message"]["content"].as_array() else {
            return;
        };
        let is_delta = v.get("timestamp_ms").is_some()
            && !raw_blocks.is_empty()
            && raw_blocks.len() <= MAX_BLOCKS_PER_LINE
            && raw_blocks
                .iter()
                .all(|b| b["type"].as_str() == Some("text"));
        if is_delta {
            for b in raw_blocks.iter().take(MAX_BLOCKS_PER_LINE) {
                if let Some(text) = b["text"].as_str()
                    && !text.is_empty()
                {
                    push_capped(&mut self.text_buf, text, MAX_ACCUM_BYTES);
                    self.emit(
                        out,
                        Event::Delta {
                            kind: DeltaKind::Text,
                            text: text.to_owned(),
                        },
                    );
                }
            }
            return;
        }
        // Final (un-timestamped) assistant message supersedes the accumulated text deltas.
        self.text_buf.clear();
        let blocks: Vec<Block> = raw_blocks
            .iter()
            .take(MAX_BLOCKS_PER_LINE)
            .filter_map(parse_block)
            .collect();
        if !blocks.is_empty() {
            self.emit(out, Event::Assistant { content: blocks });
        }
    }

    fn on_result(&mut self, v: &Value, out: &mut Vec<Event>) {
        if !self.init_emitted
            && let Some(id) = v["session_id"].as_str().filter(|s| !s.is_empty())
        {
            self.session_id = Some(id.to_owned());
            self.init_emitted = true;
            let model = self.model.clone().unwrap_or_default();
            self.emit(
                out,
                Event::Init {
                    session_id: id.to_owned(),
                    model,
                },
            );
        }
        self.flush_thinking(out);
        self.flush_text(out);
        let subtype = v["subtype"].as_str().unwrap_or_default();
        let mut is_error = v["is_error"].as_bool().unwrap_or(subtype == "error");
        let mut result = v["result"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .map(str::to_owned);
        if subtype == "interrupted" || self.interrupted {
            is_error = true;
            result = Some("interrupted".into());
        }
        if self.resuming
            && is_error
            && let Some(msg) = result.as_deref()
            && is_cursor_session_gone(msg)
        {
            self.emit(out, Event::SessionGone);
        }
        let duration_ms = v["duration_ms"].as_u64().unwrap_or(0);
        let total_cost_usd = v["total_cost_usd"].as_f64().unwrap_or(0.0);
        let usage = parse_usage(&v["usage"]);
        self.result_emitted = true;
        self.emit(
            out,
            Event::Result {
                is_error,
                duration_ms,
                total_cost_usd,
                result,
                prompts: 1,
                usage,
                turn_cost_usd: None,
            },
        );
    }
}

fn push_capped(buf: &mut String, s: &str, cap: usize) {
    let remaining = cap.saturating_sub(buf.len());
    if remaining == 0 {
        return;
    }
    if s.len() <= remaining {
        buf.push_str(s);
    } else {
        let mut cut = remaining;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        buf.push_str(&s[..cut]);
    }
}

fn parse_block(b: &Value) -> Option<Block> {
    match b["type"].as_str()? {
        "text" => Some(Block::Text {
            text: b["text"].as_str().unwrap_or_default().to_owned(),
        }),
        "thinking" => Some(Block::Thinking {
            thinking: b["thinking"]
                .as_str()
                .or_else(|| b["text"].as_str())
                .unwrap_or_default()
                .to_owned(),
        }),
        "tool_use" => Some(Block::ToolUse {
            id: b["id"].as_str().unwrap_or_default().to_owned(),
            name: b["name"].as_str().unwrap_or_default().to_owned(),
            input: b.get("input").cloned().unwrap_or(Value::Null),
        }),
        _ => None,
    }
}

fn extract_tool_start(v: &Value) -> (String, Value) {
    if let Some(obj) = v["tool_call"].as_object()
        && let Some((key, inner)) = obj.iter().next()
    {
        let args = inner.get("args").cloned().unwrap_or_else(|| json!({}));
        let name = normalize_tool_name(key, &args);
        return (name, args);
    }
    let name = v["tool_name"]
        .as_str()
        .or_else(|| v["name"].as_str())
        .unwrap_or("tool")
        .to_owned();
    let input = v
        .get("arguments")
        .or_else(|| v.get("args"))
        .or_else(|| v.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    (name, input)
}

fn normalize_tool_name(key: &str, args: &Value) -> String {
    match key {
        "shellToolCall" => "Bash".into(),
        "readToolCall" => "Read".into(),
        "writeToolCall" => "Write".into(),
        "editToolCall" => "Edit".into(),
        "deleteToolCall" => "Delete".into(),
        "lsToolCall" => "LS".into(),
        "grepToolCall" => "Grep".into(),
        "globToolCall" => "Glob".into(),
        "mcpToolCall" => {
            let server = args["serverName"]
                .as_str()
                .or_else(|| args["server"].as_str());
            let tool = args["toolName"].as_str().or_else(|| args["tool"].as_str());
            match (server, tool) {
                (Some(s), Some(t)) if !s.is_empty() && !t.is_empty() => format!("mcp__{s}__{t}"),
                _ => "mcp".into(),
            }
        }
        other => other.to_owned(),
    }
}

fn extract_tool_result(v: &Value, failed_subtype: bool) -> (Value, bool) {
    if let Some(obj) = v["tool_call"].as_object()
        && let Some((_, inner)) = obj.iter().next()
        && let Some(res) = inner.get("result")
    {
        if let Some(success) = res.get("success").filter(|val| !val.is_null()) {
            return (success.clone(), failed_subtype);
        }
        if let Some(err) = res
            .get("error")
            .filter(|val| !val.is_null())
            .or_else(|| res.get("rejected").filter(|val| !val.is_null()))
        {
            return (err.clone(), true);
        }
        if !res.is_null() {
            return (res.clone(), failed_subtype);
        }
    }
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return (err.clone(), true);
    }
    if let Some(res) = v.get("result") {
        return (res.clone(), failed_subtype);
    }
    (Value::Null, failed_subtype)
}

fn parse_usage(v: &Value) -> Option<Usage> {
    let input = v["input_tokens"]
        .as_u64()
        .or_else(|| v["inputTokens"].as_u64())?;
    let output = v["output_tokens"]
        .as_u64()
        .or_else(|| v["outputTokens"].as_u64())
        .unwrap_or(0);
    Some(Usage { input, output })
}

fn is_cursor_session_gone(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("no conversation found")
        || lower.contains("session not found")
        || lower.contains("chat not found")
}
