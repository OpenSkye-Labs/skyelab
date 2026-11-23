//! Codex's notifications and items, mapped onto [`Event`]s like Claude's: messages, tool calls and their results,
//! streaming deltas, the turn's plan (as a TodoWrite list) and rate limits.

use super::Codex;
use crate::{Block, DeltaKind, Event};
use serde_json::{Value, json};

pub(super) const NOISE: &[&str] = &[
    "item/",
    "thread/",
    "turn/",
    "account/",
    "mcpServer/",
    "remoteControl/",
    "serverRequest/",
];

/// A server request SkyeLab has no UI for (secret user input, MCP elicitation, old-style approvals): unsupported, so codex
/// treats it as refused.
pub(super) fn answer(method: &str, id: Value) -> String {
    json!({ "id": id, "error": { "code": -32601, "message": format!("SkyeLab doesn't handle {method}") } })
        .to_string()
}

pub(super) fn delta(kind: DeltaKind, p: &Value) -> Vec<Event> {
    p["delta"]
        .as_str()
        .map(|text| Event::Delta {
            kind,
            text: text.to_owned(),
        })
        .into_iter()
        .collect()
}

/// A turn's plan, shown as a TodoWrite list. One id per turn, so each update replaces the turn's list.
pub(super) fn plan(p: &Value) -> Option<Event> {
    let todos: Vec<Value> = p["plan"]
        .as_array()?
        .iter()
        .map(|s| json!({ "content": s["step"], "status": s["status"] }))
        .collect();
    Some(Event::Assistant {
        content: vec![Block::ToolUse {
            id: format!("plan-{}", p["turnId"].as_str().unwrap_or_default()),
            name: crate::TODO_TOOL.to_owned(),
            input: json!({ "todos": todos }),
        }],
    })
}

/// Codex's rate limit windows: limited once it says a limit was reached, or a window is used up; it resets when
/// the latest used-up window does. (Recorded transcripts only show windows under 100%, so "reached" is unconfirmed.)
pub(super) fn rate_limit(r: &Value) -> Event {
    let windows = [&r["primary"], &r["secondary"]];
    let full = windows
        .iter()
        .filter(|w| w["usedPercent"].as_f64().is_some_and(|u| u >= 100.0));
    let resets_at = full.clone().filter_map(|w| w["resetsAt"].as_i64()).max();
    Event::RateLimit {
        window: None,
        used: None,
        limited: !r["rateLimitReachedType"].is_null() || full.count() > 0,
        resets_at: resets_at.or(r["primary"]["resetsAt"].as_i64()),
    }
}

pub(super) fn failed(message: String) -> Event {
    Event::Result {
        is_error: true,
        duration_ms: 0,
        total_cost_usd: 0.0,
        result: Some(message),
        prompts: 1,
        usage: None,
    }
}

pub(super) fn started(item: &Value) -> Option<Event> {
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

pub(super) fn completed(item: &Value, raw: &Value) -> Option<Event> {
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
        "reasoning" => {
            let summary: Vec<&str> = item["summary"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let thinking = summary.join("\n\n");
            return (!thinking.trim().is_empty()).then(|| Event::Assistant {
                content: vec![Block::Thinking { thinking }],
            });
        }
        // Already shown (the prompt), or nothing to show.
        "userMessage" | "contextCompaction" => return None,
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

impl Codex {
    /// A user-input request, shown as an AskUserQuestion ask, each question single-choice.
    pub(super) fn on_question(&mut self, rpc: Value, params: &Value) -> (Vec<Event>, Vec<String>) {
        let mut ids = Vec::new();
        let questions: Vec<Value> = params["questions"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|q| {
                ids.push(q["id"].as_str().unwrap_or_default().to_owned());
                json!({
                    "question": q["question"],
                    "header": q["header"],
                    "options": q["options"],
                    "multiSelect": false,
                })
            })
            .collect();
        let id = format!("{}codex-{}", self.nonce, rpc);
        self.asks.insert(id.clone(), rpc);
        self.questions.insert(id.clone(), ids);
        let event = Event::Permission {
            id,
            tool: crate::QUESTION_TOOL.to_owned(),
            input: json!({ "questions": questions }),
        };
        (vec![event], Vec::new())
    }

    /// The reply to user-input request `id`: `{answers: {<question id>: {answers: [labels]}}}`, or `None` when it
    /// isn't a waiting question or the count doesn't match.
    pub(crate) fn answer_questions(&mut self, id: &str, answers: &[Vec<String>]) -> Option<String> {
        let rpc = self.asks.get(id)?;
        let ids = self
            .questions
            .get(id)
            .filter(|q| q.len() == answers.len())?;
        let map: serde_json::Map<String, Value> = ids
            .iter()
            .zip(answers)
            .map(|(q, a)| (q.clone(), json!({ "answers": a })))
            .collect();
        Some(json!({ "id": rpc, "result": { "answers": map } }).to_string())
    }
}
