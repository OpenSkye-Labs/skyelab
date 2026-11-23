//! What a thread shows, built from what it stores: the user's prompts and the agent's events.

use anyhow::{Context as _, Result};
use serde_json::Value;
use skyelab_agent::{Block, Decision, Event};

/// One stored thread event. The store's `kind` column says which.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Prompt(String),
    Agent(Event),
}

const PROMPT: &str = "prompt";
const AGENT: &str = "agent";

impl Record {
    /// The store row: `(kind, payload_json)`.
    pub fn to_row(&self) -> (&'static str, String) {
        match self {
            Record::Prompt(text) => (PROMPT, Value::String(text.clone()).to_string()),
            Record::Agent(event) => (
                AGENT,
                serde_json::to_string(event)
                    .expect("an Event holds only strings, numbers and JSON values"),
            ),
        }
    }

    pub fn from_row(kind: &str, payload: &str) -> Result<Record> {
        match kind {
            PROMPT => Ok(Record::Prompt(
                serde_json::from_str(payload).context("stored prompt")?,
            )),
            AGENT => Ok(Record::Agent(
                serde_json::from_str(payload).context("stored agent event")?,
            )),
            other => anyhow::bail!("unknown stored event kind '{other}'"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolItem {
    pub id: String,
    pub name: String,
    /// The input as pretty JSON.
    pub input: String,
    pub result: Option<String>,
    pub is_error: bool,
}

/// A tool call the agent asked permission for.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovalItem {
    /// The request id the answer names.
    pub id: String,
    pub tool: String,
    /// One line: the command, or the file it touches.
    pub summary: String,
    /// The input as pretty JSON.
    pub input: String,
    /// `None` while the agent waits for an answer.
    pub decision: Option<Decision>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Prompt(String),
    /// Assistant text, as markdown.
    Text(String),
    Tool(ToolItem),
    Done {
        is_error: bool,
        duration_ms: u64,
        cost_usd: f64,
    },
    Approval(ApprovalItem),
    /// What the agent wrote to stderr; `id` keys its expanded state.
    Output {
        id: String,
        text: String,
    },
}

impl Item {
    /// The key a row's expanded state is kept under, for rows that expand.
    pub fn expand_id(&self) -> Option<&str> {
        match self {
            Item::Tool(t) => Some(&t.id),
            Item::Output { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Applies one record. Returns the first index that changed (items from there on may be new or
/// updated), or `None` when nothing visible changed.
pub fn apply(items: &mut Vec<Item>, record: &Record) -> Option<usize> {
    let before = items.len();
    let mut first_changed = None;
    match record {
        Record::Prompt(text) => items.push(Item::Prompt(text.clone())),
        Record::Agent(Event::Assistant { content }) => {
            for block in content {
                match block {
                    Block::Text { text } if !text.trim().is_empty() => {
                        items.push(Item::Text(text.clone()))
                    }
                    Block::ToolUse { id, name, input } => items.push(Item::Tool(ToolItem {
                        id: id.clone(),
                        name: name.clone(),
                        input: serde_json::to_string_pretty(input).unwrap_or_default(),
                        result: None,
                        is_error: false,
                    })),
                    _ => {}
                }
            }
        }
        Record::Agent(Event::User { content }) => {
            for block in content {
                let Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } = block
                else {
                    continue;
                };
                // Results answer recent calls, so search from the end.
                let row = items
                    .iter_mut()
                    .enumerate()
                    .rev()
                    .find_map(|(ix, i)| match i {
                        Item::Tool(t) if t.id == *tool_use_id => Some((ix, t)),
                        _ => None,
                    });
                if let Some((ix, tool)) = row {
                    tool.result = Some(result_text(content));
                    tool.is_error = *is_error;
                    first_changed = Some(first_changed.map_or(ix, |f: usize| f.min(ix)));
                }
            }
        }
        Record::Agent(Event::Result {
            is_error,
            duration_ms,
            total_cost_usd,
            ..
        }) => items.push(Item::Done {
            is_error: *is_error,
            duration_ms: *duration_ms,
            cost_usd: *total_cost_usd,
        }),
        Record::Agent(Event::Permission { id, tool, input }) => {
            items.push(Item::Approval(ApprovalItem {
                id: id.clone(),
                tool: tool.clone(),
                summary: skyelab_agent::summary(input),
                input: serde_json::to_string_pretty(input).unwrap_or_default(),
                decision: None,
            }))
        }
        Record::Agent(Event::PermissionResolved { id, decision }) => {
            if let Some((ix, a)) = items
                .iter_mut()
                .enumerate()
                .rev()
                .find_map(|(ix, i)| match i {
                    Item::Approval(a) if a.id == *id && a.decision.is_none() => Some((ix, a)),
                    _ => None,
                })
            {
                a.decision = Some(*decision);
                first_changed = Some(ix);
            }
        }
        Record::Agent(Event::Stderr { text }) => items.push(Item::Output {
            id: format!("output-{before}"),
            text: text.clone(),
        }),
        Record::Agent(Event::Init { .. } | Event::Unknown(_)) => {}
    }
    if items.len() > before {
        first_changed = Some(first_changed.map_or(before, |f| f.min(before)));
    }
    first_changed
}

/// A tool result's content is a string or an array of blocks; keep the text.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
