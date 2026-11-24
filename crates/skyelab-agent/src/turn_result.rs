//! A delegated thread's last finished turn as data (T3's typed turn result, #38874): what `wait_thread` hands the
//! parent, so it reads status, summary, files and cost without parsing the child's reply.

use crate::Event;
use crate::handoff::edited_paths;
use crate::timeline::Record;
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
    /// Files the turn's edit tools touched, first touch first, each once.
    pub files_changed: Vec<String>,
}

/// The last turn in `records` that has its result; `None` before the first one ends.
pub fn last_turn(records: &[Record]) -> Option<TurnResult> {
    let mut last = None;
    let mut files: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut previous = Some(0.0);
    for r in records {
        let Record::Agent(event) = r else { continue };
        match event {
            Event::Assistant { content } => {
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
            Event::Result {
                is_error,
                duration_ms,
                total_cost_usd,
                result,
                ..
            } => {
                let summary = result
                    .as_deref()
                    .filter(|r| !r.trim().is_empty())
                    .unwrap_or(&text);
                last = Some(TurnResult {
                    is_error: *is_error,
                    summary: tail(summary),
                    duration_ms: *duration_ms,
                    cost_usd: crate::turn_cost(previous, *total_cost_usd),
                    files_changed: std::mem::take(&mut files),
                });
                text.clear();
                // The baseline moves right after each result, as the timeline's replay does.
                if *total_cost_usd > 0.0 {
                    previous = Some(*total_cost_usd);
                }
            }
            _ => {}
        }
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
