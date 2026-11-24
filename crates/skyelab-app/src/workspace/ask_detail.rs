//! What a permission card shows under its one-line summary: the input as JSON, a plain command's description, or a
//! file edit's diff (T3 shows the change, not its JSON).

use super::changes_model::LineKind;
use super::thread_items::clip;
use super::thread_model::ApprovalItem;
use serde_json::Value;
use skyelab_agent::timeline::cards::line_diff;

/// How many diff lines a file-change approval shows; View all opens the rest.
pub(super) const PREVIEW_LINES: usize = 20;

/// What a permission card shows under its one-line summary.
#[derive(Debug, PartialEq)]
pub(super) enum Detail {
    /// The input as JSON: anything beyond a plain command or file edit, so nothing it asks for is hidden.
    Json,
    /// A plain command's description (the command itself is the summary).
    Note(String),
    /// A Write, Edit or MultiEdit as a diff (the file path is the summary).
    Diff(Preview),
    Nothing,
}

/// A file change's first lines as a diff.
#[derive(Debug, PartialEq)]
pub(super) struct Preview {
    pub(super) lines: Vec<(Option<u32>, Option<u32>, LineKind, String)>,
    pub(super) added: usize,
    pub(super) removed: usize,
    /// Lines after `lines`.
    pub(super) more: usize,
    /// Where each MultiEdit edit after the first starts in `lines` (the card draws a divider there).
    pub(super) breaks: Vec<usize>,
    /// An Edit that replaces every match, not just one.
    pub(super) replace_all: bool,
}

/// Characters that, drawn raw, could hide part of the input: control characters (the JSON escapes them) and
/// invisible format characters (zero-width, direction and tag marks, variation selectors, line separators). `text` allows newlines, tabs and CRLF, as file contents have them.
fn hides(s: &str, text: bool) -> bool {
    let b = s.as_bytes();
    s.char_indices().any(|(i, c)| match c {
        '\n' | '\t' if text => false,
        '\r' if text => b.get(i + 1) != Some(&b'\n'),
        '\u{00AD}'
        | '\u{061C}'
        | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{2028}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E01EF}' => true,
        c => c.is_control(),
    })
}

/// `limit`: the most diff lines to give (the card's collapsed or expanded size).
pub(super) fn detail(a: &ApprovalItem, limit: usize) -> Detail {
    let Some(fields) = a.raw.as_object() else {
        return Detail::Json;
    };
    if matches!(a.tool.as_str(), "Write" | "Edit" | "MultiEdit") {
        return diff(a, fields, limit).map_or(Detail::Json, Detail::Diff);
    }
    let plain = fields.keys().all(|k| k == "command" || k == "description")
        && fields.get("command").and_then(|c| c.as_str()) == Some(a.summary.as_str())
        && !hides(&a.summary, false);
    match fields.get("description").and_then(|d| d.as_str()) {
        _ if !plain => Detail::Json,
        Some(d) if !d.trim().is_empty() => Detail::Note(clip(d)),
        _ => Detail::Nothing,
    }
}

/// A file change as a diff, or `None` when the JSON must show: a field the diff doesn't draw, a summary that isn't
/// the whole path, or text that could hide something.
fn diff(
    a: &ApprovalItem,
    fields: &serde_json::Map<String, Value>,
    limit: usize,
) -> Option<Preview> {
    let path = fields.get("file_path")?.as_str()?;
    if path != a.summary || hides(path, false) {
        return None;
    }
    let text = |o: &serde_json::Map<String, Value>, k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .is_some_and(|s| !hides(s, true))
    };
    let flag = |o: &serde_json::Map<String, Value>| {
        o.get("replace_all").map_or(Some(false), Value::as_bool)
    };
    // One Edit's fields; MultiEdit has a list of them.
    let edit = |o: &serde_json::Map<String, Value>, extra: &str| {
        o.keys().all(|k| {
            matches!(k.as_str(), "old_string" | "new_string" | "replace_all") || k == extra
        }) && text(o, "old_string")
            && text(o, "new_string")
            // A change only to line endings or a last newline would draw as no change at all.
            && {
                let s = |k| o.get(k).and_then(Value::as_str).unwrap_or_default();
                s("old_string") == s("new_string")
                    || !s("old_string").lines().eq(s("new_string").lines())
            }
    };
    let replace_all = match a.tool.as_str() {
        "Write" => (fields.keys().all(|k| k == "file_path" || k == "content")
            && text(fields, "content"))
        .then_some(false)?,
        "Edit" => edit(fields, "file_path").then(|| flag(fields))??,
        _ => {
            let edits = fields.get("edits")?.as_array()?;
            let mut any = false;
            for e in edits {
                let e = e.as_object()?;
                if !edit(e, "") {
                    return None;
                }
                any |= flag(e)?;
            }
            (fields.len() == 2).then_some(any)?
        }
    };
    let mut breaks = Vec::new();
    let mut lines = if a.tool == "MultiEdit" {
        let s = |e: &Value, k: &str| e[k].as_str().unwrap_or_default().to_owned();
        let mut out = Vec::new();
        for e in fields["edits"].as_array().into_iter().flatten() {
            if !out.is_empty() && breaks.last() != Some(&out.len()) {
                breaks.push(out.len());
            }
            out.extend(line_diff(&s(e, "old_string"), &s(e, "new_string")));
        }
        out
    } else {
        skyelab_agent::timeline::cards::input_lines(&a.tool, &a.raw, limit)
    };
    let (added, removed, total) = if a.tool == "Write" {
        let n = fields["content"].as_str().map_or(0, |c| c.lines().count());
        (n, 0, n)
    } else {
        let count = |k| lines.iter().filter(|l| l.2 == k).count();
        (
            count(LineKind::Added),
            count(LineKind::Removed),
            lines.len(),
        )
    };
    lines.truncate(limit);
    breaks.retain(|&b| b < lines.len());
    Some(Preview {
        more: total - lines.len(),
        breaks,
        lines,
        added,
        removed,
        replace_all,
    })
}

/// The whole change as a unified patch for View all (gpui-component's diff viewer): a Write as a new file, each
/// edit as its own section. `None` unless the card shows it as a diff, so the same checks cover every line.
pub(super) fn patch(a: &ApprovalItem) -> Option<String> {
    let Detail::Diff(p) = detail(a, usize::MAX) else {
        return None;
    };
    let path = &a.summary;
    let mut out = String::new();
    let mut starts = std::iter::once(0)
        .chain(p.breaks.iter().copied())
        .peekable();
    while let Some(start) = starts.next() {
        let end = starts.peek().copied().unwrap_or(p.lines.len());
        let part = &p.lines[start..end];
        if part.is_empty() {
            continue;
        }
        let old = part.iter().filter(|l| l.0.is_some()).count();
        let new = part.iter().filter(|l| l.1.is_some()).count();
        let from = if a.tool == "Write" { "/dev/null" } else { path };
        out += &format!(
            "--- {from}\n+++ {path}\n@@ -{},{old} +{},{new} @@\n",
            usize::from(old > 0),
            usize::from(new > 0)
        );
        for (_, _, kind, text) in part {
            let sign = match kind {
                LineKind::Context => ' ',
                LineKind::Added => '+',
                LineKind::Removed => '-',
            };
            out.push(sign);
            out += text;
            out.push('\n');
        }
    }
    Some(out)
}

#[cfg(test)]
#[path = "ask_detail_test.rs"]
mod tests;
