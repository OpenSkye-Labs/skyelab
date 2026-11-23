//! A subagent's card in the timeline (T3's ProviderSubagentBar and BackgroundWorkDetails): Claude's Agent (or older
//! Task) tool call, kept up to date by its `task_*` lines, with the tool calls the subagent made nested under it.

use super::thread_model::{Item, ToolItem, result_text};
use serde_json::Value;
use skyelab_agent::Block;

#[path = "subagent_card.rs"]
pub(super) mod card;
pub use card::render;

/// The tool names a subagent starts under.
pub fn is_subagent_tool(name: &str) -> bool {
    matches!(name, "Agent" | "Task")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagentItem {
    /// The Agent tool call's id: its `task_*` lines and nested messages name it.
    pub id: String,
    /// The subagent type (`general-purpose`, `Explore`), else the tool's name.
    pub kind: String,
    pub description: String,
    pub status: SubagentStatus,
    /// A `task_started` line came: the tool result alone no longer ends it (a backgrounded one answers at once).
    pub started: bool,
    /// What it is doing now (its last progress line).
    pub activity: Option<String>,
    /// Its final report: the notification's summary, else the tool result.
    pub summary: Option<String>,
    pub duration_ms: Option<u64>,
    /// The calls the subagent made, oldest first (a nested subagent's calls too).
    pub tools: Vec<ToolItem>,
}

impl SubagentItem {
    pub fn new(id: &str, name: &str, input: &Value) -> Self {
        let text = |k: &str| input[k].as_str().map(str::trim).filter(|s| !s.is_empty());
        SubagentItem {
            id: id.to_owned(),
            kind: text("subagent_type").unwrap_or(name).to_owned(),
            description: text("description")
                .or(text("prompt"))
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default()
                .to_owned(),
            status: SubagentStatus::Running,
            started: false,
            activity: None,
            summary: None,
            duration_ms: None,
            tools: Vec::new(),
        }
    }
}

/// The card a subagent's line or message belongs to: the one started by tool call `id`, or the one holding that call
/// (a subagent's own subagent rolls up into the top card). Searched from the end, where live work is.
fn card<'a>(items: &'a mut [Item], id: &str) -> Option<(usize, &'a mut SubagentItem)> {
    items
        .iter_mut()
        .enumerate()
        .rev()
        .find_map(|(ix, i)| match i {
            Item::Subagent(s) if s.id == id || s.tools.iter().any(|t| t.id == id) => Some((ix, s)),
            _ => None,
        })
}

/// A `task_*` line: the card's status, activity, report and time. Returns the row that changed.
pub fn task(
    items: &mut [Item],
    id: &str,
    status: Option<&str>,
    activity: Option<&str>,
    summary: Option<&str>,
    duration_ms: Option<u64>,
) -> Option<usize> {
    let (ix, s) = card(items, id)?;
    if s.id != id {
        return None; // a nested subagent's own line: its calls already show on the top card
    }
    match status {
        Some("running") => {
            s.started = true;
            s.status = SubagentStatus::Running;
        }
        Some("completed") => s.status = SubagentStatus::Done,
        Some(_) => s.status = SubagentStatus::Failed,
        None => {}
    }
    if let Some(a) = activity {
        s.activity = Some(a.to_owned());
    }
    if let Some(r) = summary.filter(|r| !r.trim().is_empty()) {
        s.summary = Some(r.to_owned());
    }
    s.duration_ms = duration_ms.or(s.duration_ms);
    Some(ix)
}

/// The Agent call's own result. Without `task_*` lines (an older CLI) it ends the card; a started one waits for its
/// notification, unless the result is an error.
pub fn result(items: &mut [Item], id: &str, content: &Value, is_error: bool) -> Option<usize> {
    let (ix, s) = card(items, id).filter(|(_, s)| s.id == id)?;
    if is_error {
        s.status = SubagentStatus::Failed;
    } else if !s.started {
        s.status = SubagentStatus::Done;
    }
    if s.status != SubagentStatus::Running && s.summary.is_none() {
        s.summary = Some(result_text(content));
    }
    Some(ix)
}

/// A subagent's own message: its tool calls join the card, their results fill them in.
pub fn nested(items: &mut [Item], parent: &str, content: &[Block]) -> Option<usize> {
    let (ix, s) = card(items, parent)?;
    for block in content {
        match block {
            Block::ToolUse { id, name, input } => s.tools.push(ToolItem {
                id: id.clone(),
                name: name.clone(),
                input: serde_json::to_string_pretty(input).unwrap_or_default(),
                raw: input.clone(),
                result: None,
                is_error: false,
            }),
            Block::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if let Some(t) = s.tools.iter_mut().rev().find(|t| t.id == *tool_use_id) {
                    t.result = Some(result_text(content));
                    t.is_error = *is_error;
                }
            }
            _ => {}
        }
    }
    Some(ix)
}
