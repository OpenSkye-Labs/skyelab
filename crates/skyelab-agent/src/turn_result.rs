//! A delegated thread's last finished turn as data (T3's typed turn result, #38874): what `wait_thread` hands the
//! parent, so it reads status, summary, files and cost without parsing the child's reply.

use crate::Event;
use crate::handoff::edited_paths;
use crate::timeline::{Item, Record};
use serde::Serialize;

#[cfg(test)]
#[path = "turn_result_test.rs"]
mod tests;

/// The longest summary handed back; a longer one keeps its end, where a reply's conclusion is.
pub const MAX_SUMMARY: usize = 8_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TurnResult {
    pub is_error: bool,
    /// The agent's result text, or else the turn's last assistant text.
    pub summary: String,
    pub duration_ms: u64,
    /// What this turn cost (see [`crate::turn_cost`]); 0 when unknown (Codex).
    pub cost_usd: f64,
    /// Files the edit tools touched since the previous turn that had a result (a crashed turn's edits count toward
    /// the next one), first touch first, each once.
    pub files_changed: Vec<String>,
}

/// The last turn in `records`, for a thread that is done: the last turn with its result, or, when a prompt came after
/// that result and none answered it (the agent crashed or was stopped), that turn as an error. `None` before any turn.
pub fn last_turn(records: &[Record]) -> Option<TurnResult> {
    // The timeline's own replay prices each turn, so the cost here is the Done row's (forks, imports and all).
    let mut items = crate::timeline::Items::default();
    let mut last = None;
    let mut files: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut stderr = None;
    let mut unanswered = false;
    for r in records {
        crate::timeline::apply(&mut items, r);
        match r {
            Record::Prompt(_) => unanswered = true,
            Record::Steer(_) => {}
            Record::Agent(Event::Assistant { content }) => {
                for block in content {
                    match block {
                        crate::Block::Text { text: t } if !t.trim().is_empty() => text = t.clone(),
                        crate::Block::ToolUse { name, input, .. } => {
                            for p in edited_paths(name, input) {
                                if !files.iter().any(|f| f == p) {
                                    files.push(p.to_owned());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Record::Agent(Event::Stderr { text: t }) => stderr = Some(t.clone()),
            Record::Agent(Event::Result {
                is_error,
                duration_ms,
                result,
                ..
            }) => {
                let summary = result
                    .as_deref()
                    .filter(|r| !r.trim().is_empty())
                    .unwrap_or(&text);
                let cost_usd = match items.last() {
                    Some(Item::Done { cost_usd, .. }) => *cost_usd,
                    _ => 0.0,
                };
                last = Some(TurnResult {
                    is_error: *is_error,
                    summary: tail(summary),
                    duration_ms: *duration_ms,
                    cost_usd,
                    files_changed: std::mem::take(&mut files),
                });
                text.clear();
                stderr = None;
                unanswered = false;
            }
            Record::Agent(_) => {}
        }
    }
    if unanswered {
        let why = stderr.unwrap_or_else(|| "The turn ended without a result.".to_owned());
        return Some(TurnResult {
            is_error: true,
            summary: tail(&why),
            duration_ms: 0,
            cost_usd: 0.0,
            files_changed: files,
        });
    }
    last
}

fn tail(s: &str) -> String {
    if s.len() <= MAX_SUMMARY {
        return s.to_owned();
    }
    let cut = s.ceil_char_boundary(s.len() - MAX_SUMMARY);
    format!("…{}", &s[cut..])
}
