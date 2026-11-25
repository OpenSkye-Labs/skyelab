//! The text the timeline's cards show, shared by the desktop and the phone: AskUserQuestion's questions, the turn
//! divider and usage-limit lines, an edit's line diff and a shell command's exit and plain output.

use crate::Usage;

/// How many lines of a written file the edit card draws: a huge file is never split up every frame.
pub const DIFF_LINES: usize = 400;

/// A diff line's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One question of an AskUserQuestion input.
pub struct Question {
    pub header: String,
    pub question: String,
    pub options: Vec<(String, String)>,
    pub multi: bool,
}

pub fn questions(input: &serde_json::Value) -> Vec<Question> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|q| Question {
            header: q["header"].as_str().unwrap_or_default().to_owned(),
            question: q["question"].as_str().unwrap_or_default().to_owned(),
            options: q["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    let label = o["label"].as_str()?.to_owned();
                    Some((
                        label,
                        o["description"].as_str().unwrap_or_default().to_owned(),
                    ))
                })
                .collect(),
            multi: q["multiSelect"] == true,
        })
        .collect()
}

/// The verb a finished turn's divider starts with. The agent crate marks a turn the user stopped with the error text
/// `interrupted` (both agents end it as an error), so it reads as stopped, not failed.
pub fn done_verb(is_error: bool, error: Option<&str>) -> &'static str {
    match (is_error, error.map(str::trim)) {
        (false, _) => "Worked for",
        (true, Some("interrupted")) => "Stopped after",
        (true, _) => "Failed after",
    }
}

/// The turn divider's text: "Worked for 4.2s · 12.3k in · 175 out · $0.0421".
/// A turn that recorded nothing (an imported session's) reads "Worked", not "Worked for 0.0s".
pub fn divider_text(verb: &str, secs: f64, usage: Option<Usage>, cost: f64) -> String {
    if secs <= 0.0 && usage.is_none() && cost <= 0.0 {
        return verb
            .trim_end_matches(" for")
            .trim_end_matches(" after")
            .to_owned();
    }
    let tokens = usage
        .map(|u| format!(" · {} in · {} out", count(u.input), count(u.output)))
        .unwrap_or_default();
    let cost = if cost > 0.0 {
        format!(" · ${cost:.4}")
    } else {
        String::new()
    };
    format!("{verb} {secs:.1}s{tokens}{cost}")
}

/// A token count as T3 writes it: 175, 12.3k, 1.2M.
pub fn count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..999_950 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// "Limited until …": how long until the limit resets (the reset is in Unix seconds), when known.
pub fn limited_text(resets_at: Option<i64>, now: i64) -> String {
    match resets_at.map(|r| r - now) {
        Some(left) if left > 0 => {
            format!(
                "Usage limit reached. Limited until it resets in {}.",
                wait(left)
            )
        }
        Some(_) => "Usage limit reached. It should have reset; send again to retry.".into(),
        None => "Usage limit reached. Limited until it resets.".into(),
    }
}

/// `left` seconds as "2h 14m", rounded up to the minute.
pub fn wait(left: i64) -> String {
    let minutes = (left + 59) / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Whether a tool call gets the shell card.
pub fn is_shell(name: &str) -> bool {
    matches!(name, "Bash" | "shell")
}

/// Whether a tool call gets the edit card.
pub fn is_edit(name: &str) -> bool {
    matches!(name, "Edit" | "Write" | "MultiEdit")
}

/// A line diff of `old` → `new`: the lines both share at the start and end are context, the rest removed then added.
/// Numbers count from 1 within the snippet.
///
/// shortcut: a prefix/suffix diff, not an LCS; an edit that changes two far-apart spots shows the span between as
/// changed. Fine for Edit's snippets; use a real diff if whole-file rewrites need it.
pub fn line_diff(old: &str, new: &str) -> Vec<(Option<u32>, Option<u32>, LineKind, String)> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let n = |i: usize| u32::try_from(i + 1).ok();
    let mut out = Vec::new();
    for (i, l) in a[..head].iter().enumerate() {
        out.push((n(i), n(i), LineKind::Context, (*l).to_owned()));
    }
    for (i, l) in a[head..a.len() - tail].iter().enumerate() {
        out.push((n(head + i), None, LineKind::Removed, (*l).to_owned()));
    }
    for (i, l) in b[head..b.len() - tail].iter().enumerate() {
        out.push((None, n(head + i), LineKind::Added, (*l).to_owned()));
    }
    for i in 0..tail {
        let (x, y) = (a.len() - tail + i, b.len() - tail + i);
        out.push((n(x), n(y), LineKind::Context, a[x].to_owned()));
    }
    out
}

/// An Edit / Write / MultiEdit's lines: each edit's diff in turn, or every written line added.
pub fn edit_lines(tool: &super::ToolItem) -> Vec<(Option<u32>, Option<u32>, LineKind, String)> {
    input_lines(&tool.name, &tool.raw, DIFF_LINES)
}

/// [`edit_lines`] from a tool's name and input; a Write gives at most `limit + 1` lines.
pub fn input_lines(
    name: &str,
    raw: &serde_json::Value,
    limit: usize,
) -> Vec<(Option<u32>, Option<u32>, LineKind, String)> {
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
    match name {
        // Only what the card can draw: a huge file is never split up every frame.
        "Write" => {
            let content = s(&raw["content"]);
            let head: Vec<&str> = content.lines().take(limit.saturating_add(1)).collect();
            line_diff("", &head.join("\n"))
        }
        "MultiEdit" => raw["edits"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|e| line_diff(&s(&e["old_string"]), &s(&e["new_string"])))
            .collect(),
        _ => line_diff(&s(&raw["old_string"]), &s(&raw["new_string"])),
    }
}

/// How a finished command ended: `exit N` when the output says (unconfirmed: Claude's failed Bash results start
/// "Exit code N"), `failed` for another error, `None` for success or while it runs.
pub fn exit_status(tool: &super::ToolItem) -> Option<String> {
    let result = tool.result.as_deref()?;
    let code = result
        .strip_prefix("Exit code ")
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .filter(|c| !c.is_empty());
    match (code, tool.is_error) {
        (Some(code), _) => Some(format!("exit {code}")),
        (None, true) => Some("failed".into()),
        (None, false) => None,
    }
}

/// Command output as a terminal would leave it: escape sequences dropped, a carriage return overwriting its line.
pub fn plain_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ params… final byte (@ to ~). OSC (titles, links): ESC ] … BEL or ESC \\. Any other
                // escape: drop the next char.
                if chars.next_if_eq(&'[').is_some() {
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                } else if chars.next_if_eq(&']').is_some() {
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                } else {
                    chars.next();
                }
            }
            '\r' if chars.peek() != Some(&'\n') => line.clear(),
            '\r' => {}
            '\n' => {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            c => line.push(c),
        }
    }
    out.push_str(&line);
    out
}
