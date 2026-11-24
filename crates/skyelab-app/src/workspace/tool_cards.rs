//! The timeline's richer rows, after T3's work log: thinking, the todo checklist, edit diffs, shell commands and
//! agent output. The question and plan cards are in `ask_cards.rs`. Plain data in, elements out; state lives in the
//! thread view.

use super::changes::diff_line;
use super::changes_model::LineKind;
use super::thread_items::{ItemsView, clip};
use super::thread_model::ToolItem;
use crate::ui;
use crate::ui::SkyeIcon;
use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, clipboard::Clipboard, h_flex, v_flex};
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
    let (total, shown) = (lines.len(), lines.len().min(DIFF_LINES));
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
    let more = (total > shown).then(|| {
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
        .children(tool.result.as_deref().filter(|_| tool.is_error).map(|why| {
            div()
                .px_3()
                .py_1p5()
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.danger))
                .child(clip(why))
        }))
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
