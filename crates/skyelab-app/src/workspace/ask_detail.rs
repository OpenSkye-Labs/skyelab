//! What a permission card shows under its one-line summary: the input as JSON, or a plain command's description.

use super::thread_items::clip;
use super::thread_model::ApprovalItem;

/// What a permission card shows under its one-line summary.
#[derive(Debug, PartialEq)]
pub(super) enum Detail {
    /// The input as JSON: anything beyond a plain command, so nothing it asks for is hidden.
    Json,
    /// A plain command's description (the command itself is the summary).
    Note(String),
    Nothing,
}

pub(super) fn detail(a: &ApprovalItem) -> Detail {
    let Some(fields) = a.raw.as_object() else {
        return Detail::Json;
    };
    let plain = fields.keys().all(|k| k == "command" || k == "description")
        && fields.get("command").and_then(|c| c.as_str()) == Some(a.summary.as_str())
        // The JSON escapes control characters; drawn raw they could hide part of the command.
        && !a.summary.chars().any(|c| {
            c.is_control()
                || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
        });
    match fields.get("description").and_then(|d| d.as_str()) {
        _ if !plain => Detail::Json,
        Some(d) if !d.trim().is_empty() => Detail::Note(clip(d)),
        _ => Detail::Nothing,
    }
}

#[cfg(test)]
#[path = "ask_detail_test.rs"]
mod tests;
