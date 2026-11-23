use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One line of `claude --output-format stream-json`. Serializes so callers can store and replay it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    Init {
        session_id: String,
        model: String,
    },
    Assistant {
        content: Vec<Block>,
    },
    /// Tool results (and other user turns with block content) fed back to the model.
    User {
        content: Vec<Block>,
    },
    Result {
        is_error: bool,
        duration_ms: u64,
        total_cost_usd: f64,
        result: Option<String>,
        /// How many of the prompts sent to the agent this result answers: claude can fold a prompt sent mid-turn
        /// into the running turn (two prompts, one result), and a background wake answers none. Rows stored
        /// before this field answered one.
        #[serde(default = "one")]
        prompts: u32,
        /// The turn's tokens, when the agent reported them.
        #[serde(default)]
        usage: Option<Usage>,
    },
    /// The agent asks to use a tool and waits until [`crate::Session::answer`] answers `id`.
    Permission {
        id: String,
        tool: String,
        input: Value,
    },
    /// Permission request `id` was answered, or withdrawn by the agent.
    PermissionResolved {
        id: String,
        decision: crate::Decision,
    },
    /// What the agent wrote to stderr, stored when a turn failed or the agent exited with an error.
    Stderr {
        text: String,
    },
    /// Text or thinking as it streams in. For display only: the complete block follows as [`Event::Assistant`],
    /// so callers don't store these.
    Delta {
        kind: DeltaKind,
        text: String,
    },
    /// The account's usage limit: `limited` while the agent is refused, until `resets_at` (Unix seconds).
    RateLimit {
        limited: bool,
        resets_at: Option<i64>,
        /// Which window this reading is (Claude's `rateLimitType`: `five_hour`, `seven_day`, …), when it says.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<String>,
        /// How much of that window is used, 0 to 1, when it says.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        used: Option<f32>,
    },
    /// The session `--resume` named doesn't exist any more (another config dir, or its history was deleted).
    SessionGone,
    /// Any line we don't model, or that doesn't parse: the raw JSON, or the raw text as a JSON string.
    Unknown(Value),
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaKind {
    Text,
    Thinking,
}

/// A turn's tokens. `input` counts cached and cache-write tokens too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        /// A string or an array of content blocks, as Claude sends it.
        #[serde(default)]
        content: Value,
        #[serde(default)]
        is_error: bool,
    },
    /// SkyeLab's mark on a message the turn cut off (Stop, a crash): its text is what had streamed by then.
    Partial,
    /// Any block type added later (redacted thinking, images).
    #[serde(other)]
    Other,
}

/// One TodoWrite item (Codex's plan steps map onto it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Todo {
    pub content: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// The tool name a todo list arrives under.
pub const TODO_TOOL: &str = "TodoWrite";

/// A TodoWrite input's items (`{"todos":[{"content","status"}]}`); items without text are skipped.
pub fn todos(input: &Value) -> Vec<Todo> {
    input["todos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let content = t["content"].as_str()?.trim();
            (!content.is_empty()).then(|| Todo {
                content: content.to_owned(),
                status: match t["status"].as_str() {
                    Some("completed") => TodoStatus::Completed,
                    Some("in_progress" | "inProgress") => TodoStatus::InProgress,
                    _ => TodoStatus::Pending,
                },
            })
        })
        .collect()
}

/// What Claude Code says, in a result's errors or on stderr, when `--resume` names a session it doesn't have.
const SESSION_GONE: &str = "No conversation found with session ID:";

/// Whether `event` says the resumed session is gone: [`Event::SessionGone`], or stderr saying so.
pub fn session_gone(event: &Event) -> bool {
    match event {
        Event::SessionGone => true,
        Event::Stderr { text } => text.contains(SESSION_GONE),
        _ => false,
    }
}

#[derive(Deserialize)]
struct Init {
    session_id: String,
    model: String,
}

#[derive(Deserialize)]
struct Message {
    message: Content,
}

#[derive(Deserialize)]
struct Content {
    content: Vec<Block>,
}

#[derive(Deserialize)]
struct Outcome {
    is_error: bool,
    duration_ms: u64,
    #[serde(default)]
    total_cost_usd: f64,
    result: Option<String>,
}

impl Event {
    /// Never fails: anything unexpected becomes `Unknown`, and so does a line [`Event::from_claude`] drops.
    pub fn parse(line: &str) -> Event {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Event::Unknown(Value::String(line.to_owned()));
        };
        Event::from_claude(v.clone()).unwrap_or(Event::Unknown(v))
    }

    /// One parsed stream-json line, or `None` for streaming chatter nobody shows (partial-message frames other
    /// than text and thinking, thinking token counts, status pings), which would otherwise be stored as `Unknown`.
    pub(crate) fn from_claude(v: Value) -> Option<Event> {
        let typed = match (v["type"].as_str(), v["subtype"].as_str()) {
            (Some("system"), Some("init")) => Init::deserialize(&v).ok().map(|i| Event::Init {
                session_id: i.session_id,
                model: i.model,
            }),
            (Some("system"), Some("thinking_tokens" | "status")) => return None,
            (Some("stream_event"), _) => return delta(&v),
            (Some("rate_limit_event"), _) => rate_limit(&v["rate_limit_info"]),
            (Some("assistant"), _) => Message::deserialize(&v).ok().map(|m| Event::Assistant {
                content: m.message.content,
            }),
            (Some("user"), _) => Message::deserialize(&v).ok().map(|m| Event::User {
                content: m.message.content,
            }),
            (Some("result"), _) => Outcome::deserialize(&v).ok().map(|o| Event::Result {
                is_error: o.is_error,
                duration_ms: o.duration_ms,
                total_cost_usd: o.total_cost_usd,
                result: o.result,
                prompts: 1,
                usage: v.get("usage").filter(|u| u.is_object()).map(claude_usage),
            }),
            _ => None,
        };
        Some(typed.unwrap_or(Event::Unknown(v)))
    }

    /// A usage window's reading: its label and how much is used (0 to 1). `None` for anything else, and for the
    /// many rate-limit lines that carry only a status.
    pub fn rate_limit(&self) -> Option<(&'static str, f32)> {
        let Event::RateLimit {
            window: Some(window),
            used: Some(used),
            ..
        } = self
        else {
            return None;
        };
        let label = match window.as_str() {
            "five_hour" => "Session",
            "seven_day" => "Weekly",
            "seven_day_opus" => "Weekly · Opus",
            "seven_day_sonnet" => "Weekly · Sonnet",
            _ => return None,
        };
        Some((label, *used))
    }
}

/// Whether a failed result line says the resumed session is gone, in its `errors`. A successful result's text is
/// the model's own words, which may quote anything.
pub(crate) fn names_gone_session(v: &Value) -> bool {
    v["is_error"] == true
        && v["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|e| e.starts_with(SESSION_GONE))
}

/// A partial-message frame: the root agent's text and thinking deltas. A subagent's (it has a parent tool use)
/// and every other frame (block starts and stops, tool input JSON, signatures, usage) are dropped.
fn delta(v: &Value) -> Option<Event> {
    if !v["parent_tool_use_id"].is_null() {
        return None;
    }
    let d = &v["event"]["delta"];
    let (kind, text) = match d["type"].as_str()? {
        "text_delta" => (DeltaKind::Text, d["text"].as_str()?),
        "thinking_delta" => (DeltaKind::Thinking, d["thinking"].as_str()?),
        _ => return None,
    };
    Some(Event::Delta {
        kind,
        text: text.to_owned(),
    })
}

/// Claude's `rate_limit_info`. Rejected counts as limited unless overage still lets the agent work.
fn rate_limit(info: &Value) -> Option<Event> {
    let overage = matches!(
        info["overageStatus"].as_str(),
        Some("allowed" | "allowed_warning")
    ) || info["isUsingOverage"] == true
        || info["overageInUse"] == true;
    Some(Event::RateLimit {
        limited: info["status"].as_str()? == "rejected" && !overage,
        resets_at: info["resetsAt"].as_i64(),
        window: info["rateLimitType"].as_str().map(str::to_owned),
        used: info["utilization"]
            .as_f64()
            .filter(|u| u.is_finite())
            .map(|u| u.clamp(0.0, 1.0) as f32),
    })
}

fn claude_usage(u: &Value) -> Usage {
    let n = |k: &str| u[k].as_u64().unwrap_or_default();
    Usage {
        input: n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens"),
        output: n("output_tokens"),
    }
}
