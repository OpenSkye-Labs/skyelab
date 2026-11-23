//! The timeline's richer rows, after T3's work log: thinking, the todo checklist, edit diffs, shell commands, the
//! AskUserQuestion and ExitPlanMode cards. Plain data in, elements out; state lives in the thread view.

use super::changes::diff_line;
use super::changes_model::LineKind;
use super::thread_items::{ItemsView, clip};
use super::thread_model::ToolItem;
use crate::ui;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, IconName, clipboard::Clipboard, h_flex, v_flex};
use skyelab_agent::{Todo, TodoStatus};

/// Most diff lines an edit card draws; the Changes tab has the whole file.
const DIFF_LINES: usize = 400;
/// The 24 px icon slot every work-log row starts with.
pub(super) fn slot(icon: IconName, color: gpui::Hsla) -> impl IntoElement {
    div()
        .w(px(24.))
        .flex()
        .justify_center()
        .flex_shrink_0()
        .child(Icon::new(icon).size_4().text_color(color))
}

fn chevron(open: bool, cx: &gpui::App) -> impl IntoElement {
    Icon::new(if open {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    })
    .size_3()
    .text_color(cx.theme().muted_foreground.opacity(0.7))
}

/// A row that expands: icon, label, a muted one-line preview, chevron.
fn header<V: ItemsView>(
    id: &str,
    icon: (IconName, gpui::Hsla),
    label: SharedString,
    preview: String,
    cx: &mut Context<V>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    let key = id.to_owned();
    h_flex()
        .id(SharedString::from(format!("row-{id}")))
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        .child(slot(icon.0, icon.1))
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(preview),
        )
}

/// Thinking, collapsed to "Thought" and its first line until opened. While it streams it says "Thinking…".
pub(super) fn render_thinking<V: ItemsView>(
    this: &V,
    id: &str,
    text: &str,
    live: bool,
    cx: &mut Context<V>,
) -> AnyElement {
    let open = this.is_expanded(id);
    let theme = cx.theme();
    let color = if live {
        theme.info
    } else {
        theme.muted_foreground
    };
    let first = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .trim_matches('*')
        .to_owned();
    let label = if live { "Thinking…" } else { "Thought" };
    let body = (open && !text.is_empty()).then(|| {
        let theme = cx.theme();
        div()
            .ml_7()
            .mt_1()
            .mb_2()
            .pl_3()
            .border_l_2()
            .border_color(theme.border)
            .text_size(ui::BODY)
            .text_color(theme.muted_foreground)
            .child(clip(text))
    });
    v_flex()
        .debug_selector(|| "thinking-row".into())
        .w_full()
        .child(
            header(id, (IconName::Asterisk, color), label.into(), first, cx)
                .child(chevron(open, cx)),
        )
        .children(body)
        .into_any_element()
}

/// The turn's todo list, T3's plan card: done / total, then each step with its state.
pub(super) fn render_todo(todos: &[Todo], cx: &Context<impl ItemsView>) -> AnyElement {
    let theme = cx.theme();
    let done = todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    let rows = todos.iter().map(|t| {
        let (icon, color) = match t.status {
            TodoStatus::Completed => (IconName::CircleCheck, theme.success),
            TodoStatus::InProgress => (IconName::LoaderCircle, theme.info),
            TodoStatus::Pending => (IconName::Square, theme.muted_foreground.opacity(0.6)),
        };
        h_flex()
            .min_h(px(24.))
            .gap_2()
            .text_size(ui::BODY)
            .child(Icon::new(icon).size_3p5().text_color(color))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(t.status == TodoStatus::Completed, |d| {
                        d.line_through().text_color(theme.muted_foreground)
                    })
                    .when(t.status == TodoStatus::InProgress, |d| {
                        d.font_weight(FontWeight::MEDIUM)
                    })
                    .child(t.content.clone()),
            )
    });
    v_flex()
        .debug_selector(|| "todo-card".into())
        .w_full()
        .gap_1()
        .px_3()
        .py_2p5()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .child(
            h_flex()
                .gap_2()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(div().font_weight(FontWeight::MEDIUM).child("Todos"))
                .child(format!("{done}/{}", todos.len())),
        )
        .children(rows)
        .into_any_element()
}

/// Whether a tool call gets the shell card.
pub(super) fn is_shell(name: &str) -> bool {
    matches!(name, "Bash" | "shell")
}

/// Whether a tool call gets the edit card.
pub(super) fn is_edit(name: &str) -> bool {
    matches!(name, "Edit" | "Write" | "MultiEdit")
}

/// How a finished command ended: `exit N` when the output says (unconfirmed: Claude's failed Bash results start
/// "Exit code N"), `failed` for another error, `None` for success or while it runs.
pub(super) fn exit_status(tool: &ToolItem) -> Option<String> {
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
pub(super) fn plain_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut line = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ params… final byte (@ to ~); any other escape: drop the next char.
                if chars.next_if_eq(&'[').is_some() {
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
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

/// A shell command, T3's command block: `$ command` in mono with how it ended; opened, its output in a scroll box.
pub(super) fn render_bash<V: ItemsView>(
    this: &V,
    tool: &ToolItem,
    cx: &mut Context<V>,
) -> AnyElement {
    let open = this.is_expanded(&tool.id);
    let theme = cx.theme();
    let command = tool.raw["command"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| tool.raw["command"].to_string());
    let status = exit_status(tool);
    let (icon, color) = match (&tool.result, &status) {
        (None, _) => (IconName::LoaderCircle, theme.info),
        (Some(_), Some(_)) => (IconName::CircleAlert, theme.danger),
        (Some(_), None) => (IconName::SquareTerminal, theme.muted_foreground),
    };
    let key = tool.id.clone();
    let line = h_flex()
        .id(SharedString::from(format!("bash-{}", tool.id)))
        .gap_2()
        .px_3()
        .py_2()
        .cursor_pointer()
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        .child(Icon::new(icon).size_3p5().text_color(color))
        .child(
            div()
                .flex_shrink_0()
                .text_color(theme.muted_foreground)
                .child("$"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(theme.foreground.opacity(0.85))
                .child(command),
        )
        .children(status.map(|s| {
            div()
                .flex_shrink_0()
                .text_color(ui::ink(theme, theme.danger))
                .child(s)
        }))
        .when(tool.result.is_none(), |d| {
            d.child(
                div()
                    .flex_shrink_0()
                    .text_color(theme.info)
                    .child("running"),
            )
        })
        .child(chevron(open, cx));
    let theme = cx.theme();
    let output = tool.result.as_deref().filter(|_| open).map(|r| {
        let text = plain_output(r);
        div()
            .id(SharedString::from(format!("bash-output-{}", tool.id)))
            .debug_selector(|| "bash-output".into())
            .max_h(px(256.))
            .overflow_y_scroll()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .text_color(theme.muted_foreground)
            .child(if text.trim().is_empty() {
                "(no output)".to_owned()
            } else {
                clip(text.trim_end())
            })
    });
    v_flex()
        .debug_selector(|| "bash-card".into())
        .w_full()
        .my_1()
        .rounded(ui::RADIUS)
        .border_1()
        .border_color(theme.border)
        .bg(theme.muted.opacity(0.5))
        .overflow_hidden()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .child(line)
        .children(output)
        .into_any_element()
}

/// A line diff of `old` → `new`: the lines both share at the start and end are context, the rest removed then added.
/// Numbers count from 1 within the snippet.
///
/// shortcut: a prefix/suffix diff, not an LCS; an edit that changes two far-apart spots shows the span between as
/// changed. Fine for Edit's snippets; use a real diff if whole-file rewrites need it.
pub(super) fn line_diff(old: &str, new: &str) -> Vec<(Option<u32>, Option<u32>, LineKind, String)> {
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
pub(super) fn edit_lines(tool: &ToolItem) -> Vec<(Option<u32>, Option<u32>, LineKind, String)> {
    let raw = &tool.raw;
    let s = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
    match tool.name.as_str() {
        "Write" => line_diff("", &s(&raw["content"])),
        "MultiEdit" => raw["edits"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|e| line_diff(&s(&e["old_string"]), &s(&e["new_string"])))
            .collect(),
        _ => line_diff(&s(&raw["old_string"]), &s(&raw["new_string"])),
    }
}

/// An edit, T3's inline diff: the file (click to show it in Changes) with `+N −N`, then its lines as the Changes tab
/// draws them.
pub(super) fn render_edit<V: ItemsView>(tool: &ToolItem, cx: &mut Context<V>) -> AnyElement {
    let theme = cx.theme();
    let path = tool.raw["file_path"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let lines = edit_lines(tool);
    let added = lines.iter().filter(|l| l.2 == LineKind::Added).count();
    let removed = lines.iter().filter(|l| l.2 == LineKind::Removed).count();
    let (icon, color) = match (&tool.result, tool.is_error) {
        (None, _) => (IconName::LoaderCircle, theme.info),
        (Some(_), true) => (IconName::CircleAlert, theme.danger),
        (Some(_), false) => (IconName::FileText, theme.muted_foreground),
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
    let open_path = path.clone();
    let head = h_flex()
        .gap_2()
        .px_3()
        .h(px(32.))
        .bg(theme.muted)
        .border_b_1()
        .border_color(theme.border)
        .text_size(ui::META)
        .child(Icon::new(icon).size_3p5().text_color(color))
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child(tool.name.clone()),
        )
        .child(
            div()
                .id(SharedString::from(format!("path-{}", tool.id)))
                .debug_selector(|| "card-path".into())
                .min_w_0()
                .truncate()
                .cursor_pointer()
                .text_color(theme.link)
                .hover(|s| s.underline())
                .on_click(cx.listener(move |this, _, _, cx| this.open_path(&open_path, cx)))
                .child(name),
        )
        .child(div().flex_1())
        .child(
            h_flex()
                .gap_1()
                .font_family(theme.mono_font_family.clone())
                .child(
                    div()
                        .text_color(ui::ink(theme, theme.success))
                        .child(format!("+{added}")),
                )
                .child(
                    div()
                        .text_color(ui::ink(theme, theme.danger))
                        .child(format!("−{removed}")),
                ),
        );
    let shown = lines.len().min(DIFF_LINES);
    let rows = lines
        .into_iter()
        .take(DIFF_LINES)
        .enumerate()
        .map(|(ix, (o, n, k, t))| {
            let base = h_flex()
                .id(("edit-line", ix))
                .h(px(20.))
                .w_full()
                .whitespace_nowrap()
                .font_family(theme.mono_font_family.clone())
                .text_xs();
            diff_line(base, o, n, k, t.into(), theme)
        });
    let more = (added + removed > shown).then(|| {
        div()
            .px_3()
            .py_1()
            .text_size(ui::META)
            .italic()
            .text_color(theme.muted_foreground)
            .child("More lines: open the file in Changes.")
    });
    v_flex()
        .debug_selector(|| "edit-card".into())
        .w_full()
        .my_1()
        .rounded(ui::RADIUS)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(head)
        .child(
            v_flex()
                .id(SharedString::from(format!("edit-{}", tool.id)))
                .max_h(px(384.))
                .overflow_y_scroll()
                .bg(theme.background)
                .children(rows)
                .children(more),
        )
        .into_any_element()
}

/// A copy button for `text`.
pub(super) fn copy(id: impl Into<gpui::ElementId>, text: impl Into<SharedString>) -> Clipboard {
    Clipboard::new(id).value(text).tooltip("Copy")
}

/// The agent's stderr, collapsed to one line until opened.
pub(super) fn render_output<V: ItemsView>(
    this: &V,
    id: &str,
    text: &str,
    cx: &mut Context<V>,
) -> AnyElement {
    let theme = cx.theme();
    let open = this.is_expanded(id);
    let first = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .to_owned();
    let key = id.to_owned();
    let header = h_flex()
        .id(SharedString::from(id.to_owned()))
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .rounded(ui::RADIUS_MD)
        .cursor_pointer()
        .text_size(ui::BODY)
        .hover(|s| s.bg(theme.accent))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx)))
        .child(
            div()
                .w(px(24.))
                .flex()
                .justify_center()
                .flex_shrink_0()
                .child(
                    Icon::new(IconName::CircleAlert)
                        .size_4()
                        .text_color(theme.danger),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::MEDIUM)
                .child("Agent output"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(first),
        )
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size_3()
            .text_color(theme.muted_foreground.opacity(0.7)),
        );
    v_flex()
        .w_full()
        .child(header)
        .when(open, |this| {
            this.child(
                div()
                    .ml_7()
                    .mt_1()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .rounded(ui::RADIUS)
                    .bg(theme.muted)
                    .border_1()
                    .border_color(theme.border)
                    .font_family(theme.mono_font_family.clone())
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(clip(text)),
            )
        })
        .into_any_element()
}
