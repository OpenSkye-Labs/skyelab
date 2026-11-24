//! What each timeline row says on the phone, as lines of text: compact versions of the desktop's cards. The model
//! is the desktop's (`skyelab_agent::timeline`); this only decides what a row shows closed and open. Every line is
//! clipped, so a huge payload from the other machine can't stall a frame.

use skyelab_agent::timeline::cards::{
    LineKind, divider_text, edit_lines, exit_status, is_edit, is_shell, limited_text, plain_output,
};
use skyelab_agent::timeline::subagent::{SubagentItem, SubagentStatus};
use skyelab_agent::timeline::{Item, ToolItem};
use skyelab_agent::{Decision, TodoStatus};

/// Output lines an open shell or tool row shows.
pub const OUTPUT_LINES: usize = 20;
/// Diff lines an edit row shows.
pub const DIFF_LINES: usize = 12;
/// Lines a text row (a reply, a prompt, thinking) shows.
const TEXT_LINES: usize = 500;
/// Characters any one line keeps.
const LINE_CHARS: usize = 2_000;

pub fn clip(line: &str) -> String {
    match line.char_indices().nth(LINE_CHARS) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.to_owned(),
    }
}

/// At most `n` lines of `text`, clipped, with a "… N more lines" tail when cut.
fn head(text: &str, n: usize) -> Vec<String> {
    let total = text.lines().count();
    let mut out: Vec<String> = text.lines().take(n).map(clip).collect();
    if total > n {
        out.push(format!("… {} more lines", total - n));
    }
    out
}

/// The image placeholders in a tool result ("Image (205 KB) not kept"): they show even while the row is closed.
fn images(result: Option<&str>) -> impl Iterator<Item = String> + '_ {
    result
        .into_iter()
        .flat_map(str::lines)
        .filter(|l| l.starts_with("Image (") || *l == "Image")
        .take(OUTPUT_LINES)
        .map(clip)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The text of `item` as drawn, closed or `open`. The first line is the row's header.
pub fn row_lines(item: &Item, open: bool) -> Vec<String> {
    match item {
        Item::Prompt(t) | Item::Steer(t) | Item::Text(t) => head(t, TEXT_LINES),
        Item::Live { text, .. } => head(text, TEXT_LINES),
        Item::Thinking { text, .. } => {
            let mut out = vec![if open { "Thinking ⌄" } else { "Thinking ›" }.to_owned()];
            if open {
                out.extend(head(text, TEXT_LINES));
            }
            out
        }
        Item::Tool(t) => tool_lines(t, open),
        Item::Subagent(s) => subagent_lines(s, open),
        Item::Todo(todos) => todos
            .iter()
            .take(OUTPUT_LINES)
            .map(|t| {
                let mark = match t.status {
                    TodoStatus::Completed => "✓",
                    TodoStatus::InProgress => "◐",
                    TodoStatus::Pending => "○",
                };
                clip(&format!("{mark} {}", t.content))
            })
            .collect(),
        Item::Done {
            is_error,
            duration_ms,
            cost_usd,
            usage,
            error,
            ..
        } => {
            let verb = if *is_error {
                "Failed after"
            } else {
                "Worked for"
            };
            let mut out = vec![divider_text(
                verb,
                *duration_ms as f64 / 1000.,
                *usage,
                *cost_usd,
            )];
            out.extend(error.iter().flat_map(|e| head(e, OUTPUT_LINES)));
            out
        }
        Item::Limit { resets_at } => vec![limited_text(*resets_at, now_secs())],
        Item::Notice { text, .. } => head(text, TEXT_LINES),
        Item::Approval(a) => match &a.rule {
            Some(rule) if a.decision == Some(Decision::AllowThread) => vec![clip(&format!(
                "{}: {} · allowed for this thread {rule}",
                a.tool, a.summary
            ))],
            Some(rule) => vec![clip(&format!(
                "{}: {} · allowed by project rule {rule}",
                a.tool, a.summary
            ))],
            None => vec![clip(&format!("{}: {}", a.tool, a.summary))],
        },
        Item::Interrupted => vec!["Interrupted".into()],
        Item::Output { text, .. } => {
            let mut out = vec![
                if open {
                    "Agent output ⌄"
                } else {
                    "Agent output ›"
                }
                .to_owned(),
            ];
            if open {
                out.extend(head(&plain_output(text), OUTPUT_LINES));
            }
            out
        }
    }
}

/// A shell command (`$ cmd · exit 1`, output when open), an edit (the file with `+N −N`, then its first lines), or
/// any other tool (its name and what it touched, its result when open).
fn tool_lines(t: &ToolItem, open: bool) -> Vec<String> {
    if is_shell(&t.name) {
        let command = t.raw["command"].as_str().unwrap_or_default();
        let mut header = format!(
            "$ {}",
            command.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        if let Some(exit) = exit_status(t) {
            header = format!("{header} · {exit}");
        }
        let mut out = vec![clip(&header)];
        if open && let Some(result) = &t.result {
            out.extend(head(&plain_output(result), OUTPUT_LINES));
        }
        return out;
    }
    if is_edit(&t.name) {
        let lines = edit_lines(t);
        let count = |k| lines.iter().filter(|l| l.2 == k).count();
        let file = t.raw["file_path"].as_str().unwrap_or_default();
        let mut out = vec![clip(&format!(
            "{} {file} +{} −{}",
            t.name,
            count(LineKind::Added),
            count(LineKind::Removed)
        ))];
        let changed: Vec<&(Option<u32>, Option<u32>, LineKind, String)> =
            lines.iter().filter(|l| l.2 != LineKind::Context).collect();
        let shown = if open { OUTPUT_LINES } else { DIFF_LINES };
        out.extend(changed.iter().take(shown).map(|(_, _, kind, text)| {
            let mark = if *kind == LineKind::Added { '+' } else { '-' };
            clip(&format!("{mark} {text}"))
        }));
        if changed.len() > shown {
            out.push(format!("… {} more lines", changed.len() - shown));
        }
        return out;
    }
    let mut out = vec![clip(&call_line(t))];
    if open {
        let result = t.result.as_deref().unwrap_or("Running…");
        out.extend(head(result, OUTPUT_LINES));
    } else {
        out.extend(images(t.result.as_deref()));
    }
    out
}

/// A tool call's name and what it touches; just the name when its input is empty.
fn call_line(t: &ToolItem) -> String {
    match t.raw.as_object() {
        Some(o) if o.is_empty() => t.name.clone(),
        _ => format!("{} {}", t.name, skyelab_agent::summary(&t.raw)),
    }
}

/// "Explore · Find callers", then "Running · Reading auth.rs" (or how it ended), then, open, its calls and report.
fn subagent_lines(s: &SubagentItem, open: bool) -> Vec<String> {
    let mut out = vec![clip(&format!("{} · {}", s.kind, s.description))];
    let status = match s.status {
        SubagentStatus::Running => "Running",
        SubagentStatus::Done => "Done",
        SubagentStatus::Failed => "Failed",
        SubagentStatus::Stopped => "Stopped",
    };
    let mut line = status.to_owned();
    if let Some(ms) = s.duration_ms {
        line.push_str(&format!(" · {:.1}s", ms as f64 / 1000.));
    }
    if let Some(activity) = &s.activity {
        line.push_str(&format!(" · {activity}"));
    }
    line.push_str(&format!(" · {} calls", s.tools.len()));
    out.push(clip(&line));
    if open {
        out.extend(
            s.tools
                .iter()
                .take(OUTPUT_LINES)
                .map(|t| clip(&call_line(t))),
        );
        out.extend(s.summary.iter().flat_map(|r| head(r, OUTPUT_LINES)));
    }
    out
}

/// Whether tapping the row opens or closes it.
pub fn opens(item: &Item) -> bool {
    matches!(
        item,
        Item::Thinking { .. } | Item::Tool(_) | Item::Subagent(_) | Item::Output { .. }
    )
}
