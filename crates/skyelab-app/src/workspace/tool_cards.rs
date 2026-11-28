//! The timeline's richer rows, after T3's work log: thinking, the todo checklist, edit diffs, shell commands and
//! agent output. The question and plan cards are in `ask_cards.rs`. Plain data in, elements out; state lives in the
//! thread view.

use super::diff_view;
use super::thread_items::{ItemsView, clip};
use super::thread_model::ToolItem;
use crate::ui;
use crate::ui::ActiveTheme as _;
use crate::ui::{Button, SkyeIcon};
use crate::ui::{Icon, h_flex, v_flex};
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use skyelab_agent::{Todo, TodoStatus};

use skyelab_agent::timeline::cards::DIFF_LINES;
/// Most diff lines an edit card draws; the Changes tab has the whole file.
pub(super) use skyelab_agent::timeline::cards::{edit_lines, is_edit, is_shell};
/// The 24 px icon slot every work-log row starts with.
pub(super) fn slot(icon: SkyeIcon, color: gpui::Hsla) -> impl IntoElement {
    div()
        .w(px(24.))
        .flex()
        .justify_center()
        .flex_shrink_0()
        .child(Icon::new(icon).size_4().text_color(color))
}

pub(super) fn chevron(open: bool, cx: &gpui::App) -> impl IntoElement {
    Icon::new(if open {
        SkyeIcon::ChevronDown
    } else {
        SkyeIcon::ChevronRight
    })
    .size_3()
    .text_color(cx.theme().muted_foreground.opacity(0.7))
}

/// A row that expands: icon, label, a muted one-line preview, chevron.
fn header<V: ItemsView>(
    id: &str,
    icon: (SkyeIcon, gpui::Hsla),
    label: SharedString,
    preview: String,
    cx: &mut Context<V>,
) -> gpui::Stateful<gpui::Div> {
    let theme = cx.theme();
    let key = id.to_owned();
    h_flex()
        .id(SharedString::from(format!("row-{id}")))
        .role(gpui::accesskit::Role::Button)
        .aria_label(if preview.is_empty() {
            label.to_string()
        } else {
            format!("{label}: {preview}")
        })
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
            header(id, (SkyeIcon::Asterisk, color), label.into(), first, cx)
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
            TodoStatus::Completed => (SkyeIcon::CircleCheck, theme.success),
            TodoStatus::InProgress => (SkyeIcon::LoaderCircle, theme.info),
            TodoStatus::Pending => (SkyeIcon::Square, theme.muted_foreground.opacity(0.6)),
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

/// An edit, T3's inline diff: the file (click to show it in Changes) with `+N −N`, then its lines (`diff_view`).
pub(super) fn render_edit<V: ItemsView + 'static>(
    this: &V,
    tool: &ToolItem,
    cx: &mut Context<V>,
) -> AnyElement {
    let live = this.tool_live(&tool.id);
    let path = tool.raw["file_path"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    // The input only grows while it streams, so its size tells a changed diff apart.
    let size: usize = ["old_string", "new_string", "content"]
        .iter()
        .map(|k| tool.raw[k].as_str().map_or(0, str::len))
        .chain(tool.raw["edits"].as_array().map(Vec::len))
        .sum();
    let p = diff_view::prepared(&format!("edit:{}:{size}", tool.id), &path, || {
        edit_lines(tool)
    });
    let (added, removed) = (p.added, p.removed);
    let theme = cx.theme();
    let (icon, color) = match (&tool.result, tool.is_error) {
        (None, _) if !live => (SkyeIcon::CircleAlert, theme.muted_foreground),
        (None, _) => (SkyeIcon::LoaderCircle, theme.info),
        (Some(_), true) => (SkyeIcon::CircleAlert, theme.danger),
        (Some(_), false) => (SkyeIcon::FileText, theme.muted_foreground),
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
                .role(gpui::accesskit::Role::Button)
                .aria_label(format!("Open {}", name))
                .debug_selector(|| "card-path".into())
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_middle()
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
    let error = tool.result.as_deref().filter(|_| tool.is_error).map(|why| {
        div()
            .px_3()
            .py_1p5()
            .text_size(ui::META)
            .text_color(ui::ink(theme, theme.danger))
            .child(clip(why))
    });
    // A Write past the lines a card holds (`input_lines` gives one more than it draws).
    let cut = tool.name == "Write" && p.lines.len() > DIFF_LINES;
    let (body, rows) = diff_view::body(this, &tool.id, p, cx);
    let theme = cx.theme();
    let all = this.is_expanded(&diff_view::all_key(&tool.id));
    let key = diff_view::all_key(&tool.id);
    let foot = (rows > diff_view::MAX_ROWS || cut).then(|| {
        h_flex()
            .px_2()
            .h(px(28.))
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .when(rows > diff_view::MAX_ROWS, |d| {
                d.child(
                    Button::new(SharedString::from(format!("edit-all-{}", tool.id)))
                        .debug_selector(|| "edit-show-all".into())
                        .ghost()
                        .xsmall()
                        .label(if all {
                            "Show less".to_owned()
                        } else {
                            "Show all".to_owned()
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx))),
                )
            })
            .when(cut, |d| d.child("The rest of the file is in Changes."))
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
        .children(error)
        .child(
            div()
                .bg(theme.background)
                .font_family(theme.mono_font_family.clone())
                .text_size(theme.mono_font_size)
                .child(body),
        )
        .children(foot)
        .into_any_element()
}

/// A copy button for `text`.
pub(super) fn copy(
    id: impl Into<gpui::ElementId>,
    text: impl Into<SharedString>,
) -> crate::ui::CopyButton {
    crate::ui::CopyButton::new(id).value(text).tooltip("Copy")
}

/// A reply's copy button. Every reply has one: its first words tell them apart.
pub(super) fn copy_reply(id: impl Into<gpui::ElementId>, text: &str) -> crate::ui::CopyButton {
    let first: Vec<&str> = text.split_whitespace().take(8).collect();
    copy(id, text.to_owned()).accessibility_label(format!("Copy reply: {}", first.join(" ")))
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
        .role(gpui::accesskit::Role::Button)
        .aria_label(if first.is_empty() {
            "Agent output".to_owned()
        } else {
            format!("Agent output: {first}")
        })
        .aria_expanded(open)
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
                    Icon::new(SkyeIcon::CircleAlert)
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
                SkyeIcon::ChevronDown
            } else {
                SkyeIcon::ChevronRight
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

/// The first argument's value, as T3 shows it (`ls crates`, not `"command": "ls crates"`).
pub(super) fn tool_summary(tool: &ToolItem) -> String {
    tool.input
        .lines()
        .filter_map(|l| l.split_once(':').map(|(_, v)| v.trim()))
        // A nested list or object (`"questions": [`) says nothing; its first plain value does.
        .find(|v| !v.starts_with(['[', '{']))
        .unwrap_or_default()
        .trim_end_matches(',')
        .trim_matches('"')
        .chars()
        .take(80)
        .collect()
}
