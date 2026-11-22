//! Layout pieces shared by the settings sections.

use gpui::{App, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, tooltip::Tooltip};
use std::path::Path;

pub(super) fn section(title: &str, cx: &App) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_md()
        .child(div().text_lg().pb_1().child(title.to_string()))
}

/// One line, middle-truncated so both the root and the folder name stay visible; full path on hover.
/// The caller sizes it (it must be allowed to shrink, hence `min_w_0`).
pub(super) fn path_text(id: String, path: &Path) -> gpui::Stateful<gpui::Div> {
    let full = SharedString::from(path.display().to_string());
    div()
        .id(SharedString::from(id))
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .child(full.clone())
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
}

pub(super) fn row(cx: &App) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .border_b_1()
        .border_color(cx.theme().border)
}
