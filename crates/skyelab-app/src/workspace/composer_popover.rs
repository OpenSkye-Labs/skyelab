//! The composer's floating surfaces: T3's popup panel and a muted hint line.

use crate::ui;
use gpui::{AnyElement, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, v_flex};

/// A floating panel above the composer card, T3's popup surface.
pub(in crate::workspace) fn popover(cx: &gpui::App) -> gpui::Div {
    let theme = cx.theme();
    v_flex()
        .absolute()
        .bottom_full()
        .mb_2()
        .p_1()
        .rounded(ui::RADIUS_LG)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_md()
}

pub(in crate::workspace) fn hint(text: impl Into<SharedString>, cx: &gpui::App) -> AnyElement {
    div()
        .px_2()
        .py_1()
        .text_size(ui::META)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
        .into_any_element()
}
