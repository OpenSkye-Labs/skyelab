//! What a thread shows, built from what it stores: the user's prompts and the agent's events.

use anyhow::{Context as _, Result};
use serde_json::Value;
use skyelab_agent::{Block, Decision, DeltaKind, Event, Todo, Usage};

/// One stored thread event. The store's `kind` column says which.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Prompt(String),
    /// A message steered into the running turn: part of that turn, not the start of a new one.
    Steer(String),
    Agent(Event),
}

const PROMPT: &str = "prompt";
const STEER: &str = "steer";
const AGENT: &str = "agent";

impl Record {
    /// The store row: `(kind, payload_json)`.
    pub fn to_row(&self) -> (&'static str, String) {
        match self {
            Record::Prompt(text) => (PROMPT, Value::String(text.clone()).to_string()),
            Record::Steer(text) => (STEER, Value::String(text.clone()).to_string()),
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
            STEER => Ok(Record::Steer(
                serde_json::from_str(payload).context("stored steer")?,
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
    /// The input as the agent sent it, for the cards that lay it out (Bash, Edit, Write).
    pub raw: Value,
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
    /// The input as the agent sent it: an AskUserQuestion's questions, an ExitPlanMode's plan.
    pub raw: Value,
    /// `None` while the agent waits for an answer.
    pub decision: Option<Decision>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Prompt(String),
    /// A message steered into the running turn; drawn like a prompt, folded with its turn.
    Steer(String),
    /// Assistant text, as markdown.
    Text(String),
    /// Text or thinking still streaming in; the complete block replaces it.
    Live {
        kind: DeltaKind,
        text: String,
    },
    /// What the agent thought; `id` keys its expanded state.
    Thinking {
        id: String,
        text: String,
    },
    Tool(ToolItem),
    /// A subagent (Claude's Agent tool) with the calls it made.
    Subagent(super::subagent::SubagentItem),
    /// The turn's todo list: every TodoWrite in a turn updates this one row.
    Todo(Vec<Todo>),
    Done {
        is_error: bool,
        duration_ms: u64,
        cost_usd: f64,
        usage: Option<Usage>,
        /// When the turn ended (ms since the epoch), once [`stamp_done`] has set it.
        at_ms: Option<i64>,
        /// A failed turn's error text, when the agent gave one.
        error: Option<String>,
    },
    /// The usage limit was hit; it resets at `resets_at` (Unix seconds), when known.
    Limit {
        resets_at: Option<i64>,
    },
    /// Something SkyeLab did that the user should know about. `explains_failure`: the next failed turn is the one
    /// this note explains, so it gets no divider of its own.
    Notice {
        text: String,
        explains_failure: bool,
    },
    Approval(ApprovalItem),
    /// The note under a message its turn cut off (Stop, a crash): the text above is what had streamed.
    Interrupted,
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
            Item::Subagent(s) => Some(&s.id),
            Item::Output { id, .. } | Item::Thinking { id, .. } => Some(id),
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
        Record::Steer(text) => items.push(Item::Steer(text.clone())),
        Record::Agent(Event::Delta { kind, text }) => match live(items, *kind) {
            Some(ix) => {
                if let Item::Live { text: t, .. } = &mut items[ix] {
                    t.push_str(text);
                }
                first_changed = Some(ix);
            }
            None => items.push(Item::Live {
                kind: *kind,
                text: text.clone(),
            }),
        },
        Record::Agent(Event::Assistant { content }) => {
            for block in content {
                let changed = match block {
                    Block::Text { text } => finish(items, DeltaKind::Text, text),
                    Block::Thinking { thinking } => finish(items, DeltaKind::Thinking, thinking),
                    Block::ToolUse { name, input, .. } if name == skyelab_agent::TODO_TOOL => {
                        todo(items, skyelab_agent::todos(input))
                    }
                    // Right after the cut-off text, even if a steer was shown below it while it streamed.
                    Block::Partial => {
                        let at = items
                            .iter()
                            .rposition(|i| matches!(i, Item::Text(_)))
                            .map_or(items.len(), |t| t + 1);
                        items.insert(at, Item::Interrupted);
                        Some(at)
                    }
                    Block::ToolUse { id, name, input }
                        if super::subagent::is_subagent_tool(name) =>
                    {
                        items.push(Item::Subagent(super::subagent::SubagentItem::new(
                            id, name, input,
                        )));
                        None
                    }
                    Block::ToolUse { id, name, input } => {
                        items.push(Item::Tool(ToolItem {
                            id: id.clone(),
                            name: name.clone(),
                            input: serde_json::to_string_pretty(input).unwrap_or_default(),
                            raw: input.clone(),
                            result: None,
                            is_error: false,
                        }));
                        None
                    }
                    _ => None,
                };
                if let Some(ix) = changed {
                    first_changed = Some(first_changed.map_or(ix, |f: usize| f.min(ix)));
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
                let changed = match row {
                    Some((ix, tool)) => {
                        tool.result = Some(result_text(content));
                        tool.is_error = *is_error;
                        Some(ix)
                    }
                    None => super::subagent::result(items, tool_use_id, content, *is_error),
                };
                if let Some(ix) = changed {
                    first_changed = Some(first_changed.map_or(ix, |f: usize| f.min(ix)));
                }
            }
        }
        Record::Agent(Event::Result {
            is_error,
            duration_ms,
            total_cost_usd,
            usage,
            result,
            ..
        }) => {
            if let Some(ix) = settle_live(items) {
                first_changed = Some(first_changed.map_or(ix, |f: usize| f.min(ix)));
            }
            // The failure that says a resumed session is gone is explained by the note just above it.
            let explained = match items.last_mut() {
                Some(Item::Notice {
                    explains_failure, ..
                }) if *is_error => std::mem::take(explains_failure),
                _ => false,
            };
            if !explained {
                items.push(Item::Done {
                    is_error: *is_error,
                    duration_ms: *duration_ms,
                    cost_usd: *total_cost_usd,
                    usage: *usage,
                    at_ms: None,
                    error: result.clone().filter(|_| *is_error),
                });
                // The finished turn folds its work rows (`timeline_fold`), so all of them change.
                let start = super::timeline_fold::turn_start(items, items.len() - 1);
                first_changed = Some(first_changed.map_or(start, |f: usize| f.min(start)));
            }
        }
        Record::Agent(Event::Permission { id, tool, input }) => {
            items.push(Item::Approval(ApprovalItem {
                id: id.clone(),
                tool: tool.clone(),
                summary: skyelab_agent::summary(input),
                input: serde_json::to_string_pretty(input).unwrap_or_default(),
                raw: input.clone(),
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
        Record::Agent(e @ (Event::SessionGone | Event::Stderr { .. }))
            if skyelab_agent::session_gone(e) =>
        {
            if !matches!(items.last(), Some(Item::Notice { .. })) {
                items.push(Item::Notice {
                    text: SESSION_GONE_NOTE.into(),
                    explains_failure: true,
                });
            }
        }
        Record::Agent(Event::RateLimit {
            limited: true,
            resets_at,
            ..
        }) => items.push(Item::Limit {
            resets_at: *resets_at,
        }),
        Record::Agent(Event::Task {
            tool_use_id,
            status,
            activity,
            summary,
            duration_ms,
        }) => {
            first_changed = super::subagent::task(
                items,
                tool_use_id,
                status.as_deref(),
                activity.as_deref(),
                summary.as_deref(),
                *duration_ms,
            );
        }
        Record::Agent(Event::Nested { parent, content }) => {
            first_changed = super::subagent::nested(items, parent, content);
        }
        Record::Agent(Event::Stderr { text }) => items.push(Item::Output {
            id: format!("output-{before}"),
            text: text.clone(),
        }),
        Record::Agent(
            Event::Init { .. } | Event::Unknown(_) | Event::RateLimit { .. } | Event::SessionGone,
        ) => {}
    }
    if items.len() > before {
        first_changed = Some(first_changed.map_or(before, |f| f.min(before)));
    }
    first_changed
}

/// What a thread says when its resumed session was gone and it went on in a new one.
pub const SESSION_GONE_NOTE: &str =
    "The agent's earlier session couldn't be found, so this continues in a new session.";

/// The thread's usage limit after `record`: `Some(resets_at)` while limited. A limited rate-limit event sets it; an
/// unlimited one, or a turn that succeeded, clears it.
pub fn limit_after(now: Option<Option<i64>>, record: &Record) -> Option<Option<i64>> {
    match record {
        Record::Agent(Event::RateLimit {
            limited, resets_at, ..
        }) => limited.then_some(*resets_at),
        Record::Agent(Event::Result {
            is_error: false, ..
        }) => None,
        _ => now,
    }
}

/// Settles rows still streaming: a block that never completed (the turn failed or was stopped) stays as it
/// streamed. Returns the first row that changed.
pub fn settle_live(items: &mut [Item]) -> Option<usize> {
    let mut first = None;
    for (ix, item) in items.iter_mut().enumerate() {
        if let Item::Live { kind, text } = item {
            let done = settled(*kind, std::mem::take(text), ix);
            *item = done;
            first = first.or(Some(ix));
        }
    }
    first
}

/// The message a turn cut off before its final one (Stop, a crash, an interrupt): the text still streaming, as an
/// assistant event marked [`Block::Partial`], so storing it keeps it. `None` when no text is streaming.
pub fn partial(items: &[Item]) -> Option<Record> {
    // The row `apply` settles, so storing it can't leave a second copy (callers store it before any stderr row).
    let Item::Live { text, .. } = &items[live(items, DeltaKind::Text)?] else {
        return None;
    };
    (!text.trim().is_empty()).then(|| {
        Record::Agent(Event::Assistant {
            content: vec![Block::Text { text: text.clone() }, Block::Partial],
        })
    })
}

/// The streaming row of `kind`, among the rows still streaming at the end. A prompt sent while it streams (a
/// follow-up) doesn't end it.
fn live(items: &[Item], kind: DeltaKind) -> Option<usize> {
    items
        .iter()
        .enumerate()
        .rev()
        .take_while(|(_, i)| matches!(i, Item::Live { .. } | Item::Prompt(_) | Item::Steer(_)))
        .find(|(_, i)| matches!(i, Item::Live { kind: k, .. } if *k == kind))
        .map(|(ix, _)| ix)
}

/// A complete text or thinking row at `ix`.
fn settled(kind: DeltaKind, text: String, ix: usize) -> Item {
    match kind {
        DeltaKind::Text => Item::Text(text),
        DeltaKind::Thinking => Item::Thinking {
            id: format!("thinking-{ix}"),
            text,
        },
    }
}

/// A complete text or thinking block: it replaces its streaming row (returned as changed), or is pushed. An empty
/// one (thinking the agent didn't show) keeps what streamed, or adds nothing.
fn finish(items: &mut Vec<Item>, kind: DeltaKind, text: &str) -> Option<usize> {
    let at = live(items, kind);
    let text = match (text.trim().is_empty(), at) {
        (false, _) => text.to_owned(),
        (true, Some(ix)) => match &mut items[ix] {
            Item::Live { text, .. } => std::mem::take(text),
            _ => String::new(),
        },
        (true, None) => return None,
    };
    match at {
        Some(ix) => {
            items[ix] = settled(kind, text, ix);
            Some(ix)
        }
        None => {
            items.push(settled(kind, text, items.len()));
            None
        }
    }
}

/// Dates the last turn divider, if `apply` just added it: `at_ms` is the event's time (now, for a live one).
pub fn stamp_done(items: &mut [Item], at_ms: i64) {
    if let Some(Item::Done {
        at_ms: at @ None, ..
    }) = items.last_mut()
    {
        *at = Some(at_ms);
    }
}

/// A TodoWrite: it updates the turn's todo row in place (returned as changed), or starts one. The turn runs from
/// the last turn divider, so a follow-up prompt sent mid-turn doesn't start a second list.
fn todo(items: &mut Vec<Item>, todos: Vec<Todo>) -> Option<usize> {
    let turn = items
        .iter()
        .rposition(|i| matches!(i, Item::Done { .. }))
        .map_or(0, |p| p + 1);
    match items[turn..]
        .iter()
        .position(|i| matches!(i, Item::Todo(_)))
    {
        Some(at) => {
            items[turn + at] = Item::Todo(todos);
            Some(turn + at)
        }
        None => {
            items.push(Item::Todo(todos));
            None
        }
    }
}

/// A tool result's content is a string or an array of blocks; keep the text.
pub(super) fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| match b["type"].as_str() {
                Some("image") => Some(skyelab_protocol::image_label(b)),
                _ => b["text"].as_str().map(str::to_owned),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
