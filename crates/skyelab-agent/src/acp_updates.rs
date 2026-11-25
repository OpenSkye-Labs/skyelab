//! Bounded translation of ACP `session/update` notifications into SkyeLab [`Event`]s.

use crate::{Block, DeltaKind, Event, TODO_TOOL};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, VecDeque};

/// Hard cap on tracked tool calls per session (FIFO eviction when exceeded).
pub(crate) const MAX_TRACKED_TOOLS: usize = 256;
/// Hard cap on entries kept from a single ACP `plan` / `plan_update`.
pub(crate) const MAX_PLAN_ENTRIES: usize = 256;
/// Hard cap on any sanitized tool text string, in bytes.
pub(crate) const MAX_TOOL_TEXT_BYTES: usize = 64 << 10;
/// Hard cap on accumulated text or thinking buffered per turn before flush.
pub(crate) const MAX_TURN_TEXT_BYTES: usize = 4 << 20;
const MAX_SANITIZE_DEPTH: usize = 12;
const MAX_SANITIZE_NODES: usize = 512;
const TRUNCATED_SUFFIX: &str = "… [truncated]";

#[derive(Default)]
pub(crate) struct TurnBuffers {
    thinking: String,
    text: String,
    pub(crate) last_text: Option<String>,
}

impl TurnBuffers {
    pub(crate) fn push_thinking<'a>(&mut self, chunk: &'a str) -> &'a str {
        append_bounded(&mut self.thinking, chunk, MAX_TURN_TEXT_BYTES)
    }

    pub(crate) fn push_text<'a>(&mut self, chunk: &'a str) -> &'a str {
        append_bounded(&mut self.text, chunk, MAX_TURN_TEXT_BYTES)
    }

    pub(crate) fn flush(&mut self) -> Option<Event> {
        let mut content = Vec::new();
        if !self.thinking.is_empty() {
            content.push(Block::Thinking {
                thinking: std::mem::take(&mut self.thinking),
            });
        }
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.last_text = Some(text.clone());
            content.push(Block::Text { text });
        }
        (!content.is_empty()).then_some(Event::Assistant { content })
    }

    pub(crate) fn clear(&mut self) {
        self.thinking.clear();
        self.text.clear();
        self.last_text = None;
    }
}

#[derive(Default)]
pub(crate) struct ToolCallState {
    emitted_use: bool,
    emitted_result: bool,
}

#[derive(Default)]
pub(crate) struct ToolCallTracker {
    order: VecDeque<String>,
    calls: BTreeMap<String, ToolCallState>,
}

impl ToolCallTracker {
    pub(crate) fn clear(&mut self) {
        self.order.clear();
        self.calls.clear();
    }

    fn entry_mut(&mut self, id: &str) -> &mut ToolCallState {
        if !self.calls.contains_key(id) {
            while self.calls.len() >= MAX_TRACKED_TOOLS {
                if let Some(oldest) = self.order.pop_front() {
                    self.calls.remove(&oldest);
                } else {
                    break;
                }
            }
            self.order.push_back(id.to_owned());
            self.calls.insert(id.to_owned(), ToolCallState::default());
        }
        self.calls.entry(id.to_owned()).or_default()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.calls.len()
    }
}

pub(crate) fn handle_update(
    update: &Value,
    tools: &mut ToolCallTracker,
    buf: &mut TurnBuffers,
) -> Vec<Event> {
    let kind = update["sessionUpdate"].as_str().unwrap_or_default();
    match kind {
        "agent_message_chunk" => {
            let Some(text) = content_text(&update["content"]) else {
                return Vec::new();
            };
            let kept = buf.push_text(&text);
            if kept.is_empty() {
                return Vec::new();
            }
            vec![Event::Delta {
                kind: DeltaKind::Text,
                text: kept.to_owned(),
            }]
        }
        "agent_thought_chunk" => {
            let Some(text) = content_text(&update["content"]) else {
                return Vec::new();
            };
            let kept = buf.push_thinking(&text);
            if kept.is_empty() {
                return Vec::new();
            }
            vec![Event::Delta {
                kind: DeltaKind::Thinking,
                text: kept.to_owned(),
            }]
        }
        "tool_call" | "tool_call_update" => {
            handle_tool_call(update, kind == "tool_call", tools, buf)
        }
        "plan" | "plan_update" => handle_plan(update, buf),
        "usage_update" => {
            let used = update["used"].as_u64();
            let size = update["size"].as_u64().filter(|&s| s > 0);
            if used.is_none() && size.is_none() {
                return Vec::new();
            }
            vec![Event::Context {
                tokens: used,
                window: size,
                model: None,
            }]
        }
        _ => Vec::new(),
    }
}

fn handle_tool_call(
    update: &Value,
    is_start: bool,
    tools: &mut ToolCallTracker,
    buf: &mut TurnBuffers,
) -> Vec<Event> {
    let Some(raw_id) = update["toolCallId"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Vec::new();
    };
    let id = truncate_utf8(raw_id, 256);
    let mut events = Vec::new();
    let status = update["status"].as_str().unwrap_or_default();
    let terminal = matches!(status, "completed" | "failed");

    let emit_use = {
        let state = tools.entry_mut(&id);
        if is_start && !state.emitted_use {
            state.emitted_use = true;
            true
        } else {
            false
        }
    };
    if emit_use {
        if let Some(a) = buf.flush() {
            events.push(a);
        }
        let name = tool_name(update);
        let input = tool_input(update);
        events.push(Event::Assistant {
            content: vec![Block::ToolUse {
                id: id.clone(),
                name,
                input,
            }],
        });
    }

    if terminal {
        let emit_result = {
            let state = tools.entry_mut(&id);
            if !state.emitted_result {
                state.emitted_result = true;
                true
            } else {
                false
            }
        };
        if emit_result {
            let content = tool_output(update);
            events.push(Event::User {
                content: vec![Block::ToolResult {
                    tool_use_id: id,
                    content,
                    is_error: status == "failed",
                }],
            });
        }
    }
    events
}

fn handle_plan(update: &Value, buf: &mut TurnBuffers) -> Vec<Event> {
    let entries = update["entries"]
        .as_array()
        .or_else(|| update["plan"]["entries"].as_array());
    let Some(entries) = entries else {
        return Vec::new();
    };
    let todos: Vec<Value> = entries
        .iter()
        .take(MAX_PLAN_ENTRIES)
        .filter_map(|e| {
            let content = e["content"].as_str()?.trim();
            if content.is_empty() {
                return None;
            }
            let status = match e["status"].as_str() {
                Some("completed" | "done") => "completed",
                Some("in_progress" | "inProgress") => "in_progress",
                _ => "pending",
            };
            Some(json!({
                "content": truncate_utf8(content, 512),
                "status": status,
            }))
        })
        .collect();
    if todos.is_empty() {
        return Vec::new();
    }
    let mut events = Vec::new();
    if let Some(a) = buf.flush() {
        events.push(a);
    }
    let id = "acp-plan".to_owned();
    events.push(Event::Assistant {
        content: vec![Block::ToolUse {
            id: id.clone(),
            name: TODO_TOOL.to_owned(),
            input: json!({ "todos": todos }),
        }],
    });
    events.push(Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: id,
            content: Value::String(String::new()),
            is_error: false,
        }],
    });
    events
}

fn tool_name(update: &Value) -> String {
    let raw = update["title"]
        .as_str()
        .or_else(|| update["name"].as_str())
        .or_else(|| update["kind"].as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Tool");
    truncate_utf8(raw, 256)
}

fn tool_input(update: &Value) -> Value {
    if !update["rawInput"].is_null() {
        return sanitize_payload(&update["rawInput"]);
    }
    if let Some(title) = update["title"].as_str().filter(|s| !s.is_empty()) {
        return json!({ "title": truncate_utf8(title, 512) });
    }
    json!({})
}

fn tool_output(update: &Value) -> Value {
    if !update["rawOutput"].is_null() {
        return sanitize_payload(&update["rawOutput"]);
    }
    if let Some(arr) = update["content"].as_array() {
        let mut text = String::new();
        for item in arr.iter().take(64) {
            if let Some(t) = content_text(&item["content"]).or_else(|| content_text(item)) {
                if !text.is_empty() {
                    text.push('\n');
                }
                append_bounded(&mut text, &t, MAX_TOOL_TEXT_BYTES);
            }
        }
        if !text.is_empty() {
            return Value::String(text);
        }
    }
    Value::String(String::new())
}

pub(crate) fn content_text(content: &Value) -> Option<String> {
    match content["type"].as_str() {
        Some("text") => content["text"]
            .as_str()
            .map(|s| truncate_utf8(s, MAX_TOOL_TEXT_BYTES)),
        Some("resource") => content["resource"]["text"]
            .as_str()
            .map(|s| truncate_utf8(s, MAX_TOOL_TEXT_BYTES)),
        Some("resource_link") => {
            let name = content["title"]
                .as_str()
                .or_else(|| content["name"].as_str())
                .unwrap_or("resource");
            Some(truncate_utf8(name, 512))
        }
        _ => None,
    }
}

/// Sanitizes untrusted JSON payloads by enforcing recursion depth, node budget, and string byte caps.
pub(crate) fn sanitize_payload(v: &Value) -> Value {
    let mut budget = MAX_SANITIZE_NODES;
    sanitize_inner(v, 0, &mut budget)
}

fn sanitize_inner(v: &Value, depth: usize, budget: &mut usize) -> Value {
    if depth >= MAX_SANITIZE_DEPTH || *budget == 0 {
        return Value::Null;
    }
    *budget -= 1;
    match v {
        Value::Null | Value::Bool(_) | Value::Number(_) => v.clone(),
        Value::String(s) => Value::String(truncate_utf8(s, MAX_TOOL_TEXT_BYTES)),
        Value::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                if *budget == 0 {
                    break;
                }
                out.push(sanitize_inner(item, depth + 1, budget));
            }
            Value::Array(out)
        }
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, val) in map {
                if *budget == 0 {
                    break;
                }
                let key = truncate_utf8(k, 256);
                out.insert(key, sanitize_inner(val, depth + 1, budget));
            }
            Value::Object(out)
        }
    }
}

pub(crate) fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_owned();
    }
    let keep = max_bytes.saturating_sub(TRUNCATED_SUFFIX.len());
    let mut end = keep.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + TRUNCATED_SUFFIX.len());
    out.push_str(&s[..end]);
    out.push_str(TRUNCATED_SUFFIX);
    out
}

fn append_bounded<'a>(dst: &mut String, chunk: &'a str, max_bytes: usize) -> &'a str {
    if dst.len() >= max_bytes || chunk.is_empty() {
        return "";
    }
    let remaining = max_bytes - dst.len();
    let kept = if chunk.len() <= remaining {
        chunk
    } else {
        let mut end = remaining;
        while end > 0 && !chunk.is_char_boundary(end) {
            end -= 1;
        }
        &chunk[..end]
    };
    dst.push_str(kept);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_caps_tracked_tools_at_max() {
        let mut tracker = ToolCallTracker::default();
        let mut buf = TurnBuffers::default();
        for i in 0..(MAX_TRACKED_TOOLS + 50) {
            let upd = json!({
                "sessionUpdate": "tool_call",
                "toolCallId": format!("call_{i}"),
                "title": "Read",
                "kind": "read",
                "status": "in_progress"
            });
            let _ = handle_update(&upd, &mut tracker, &mut buf);
        }
        assert_eq!(tracker.len(), MAX_TRACKED_TOOLS);
    }

    #[test]
    fn sanitize_bounds_depth_nodes_and_strings() {
        let mut nested = json!("leaf");
        for _ in 0..25 {
            nested = json!({ "a": nested });
        }
        let cleaned = sanitize_payload(&nested);
        // Depth 12 truncates the leaf to Null.
        let mut cur = &cleaned;
        for _ in 0..12 {
            cur = &cur["a"];
        }
        assert!(cur.is_null(), "depth capped at 12: {cleaned}");

        let huge = Value::String("x".repeat(MAX_TOOL_TEXT_BYTES + 10_000));
        let cleaned_str = sanitize_payload(&huge);
        assert!(cleaned_str.as_str().expect("str").len() <= MAX_TOOL_TEXT_BYTES);
    }

    #[test]
    fn turn_buffer_caps_accumulated_text() {
        let mut tools = ToolCallTracker::default();
        let mut buf = TurnBuffers::default();
        let chunk = "a".repeat(64 << 10);
        let update = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": chunk },
        });
        let mut delta_bytes = 0usize;
        for _ in 0..100 {
            for e in handle_update(&update, &mut tools, &mut buf) {
                if let Event::Delta { text, .. } = e {
                    delta_bytes += text.len();
                }
            }
        }
        assert_eq!(delta_bytes, MAX_TURN_TEXT_BYTES);
        let Some(Event::Assistant { content }) = buf.flush() else {
            panic!("expected assistant event");
        };
        let Block::Text { text } = &content[0] else {
            panic!("expected text block");
        };
        assert_eq!(text.len(), MAX_TURN_TEXT_BYTES);
    }
}
