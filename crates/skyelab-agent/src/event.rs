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
    },
    /// Any line we don't model, or that doesn't parse: the raw JSON, or the raw text as a JSON string.
    Unknown(Value),
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
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
    /// Thinking and any block type added later.
    #[serde(other)]
    Other,
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
    /// Never fails: anything unexpected becomes `Unknown`.
    pub fn parse(line: &str) -> Event {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return Event::Unknown(Value::String(line.to_owned()));
        };
        let typed = match (v["type"].as_str(), v["subtype"].as_str()) {
            (Some("system"), Some("init")) => Init::deserialize(&v).ok().map(|i| Event::Init {
                session_id: i.session_id,
                model: i.model,
            }),
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
            }),
            _ => None,
        };
        typed.unwrap_or(Event::Unknown(v))
    }
}
