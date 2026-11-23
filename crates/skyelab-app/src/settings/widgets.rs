//! Layout pieces shared by the settings sections.

use crate::ui;
use gpui::{App, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, tooltip::Tooltip, v_flex};
use std::path::Path;

/// A settings section: its title, then its rows in one rounded group (T3's settings layout).
pub(super) fn section(
    title: &str,
    rows: impl IntoIterator<Item = gpui::AnyElement>,
    cx: &App,
) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .gap_2p5()
        .child(ui::section_title(title.to_string(), theme))
        .child(ui::group(theme).children(rows))
}

/// Middle-truncated so both the root and the folder name stay visible; full path on hover.
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

/// A row of a [`section`]; `first` has no hairline above it.
pub(super) fn row(first: bool, cx: &App) -> gpui::Div {
    ui::group_row(first, cx.theme())
}
