//! Surfaces: `Card` (T3's card and settings group; `raised` floats over the page with a shadow) and `Divider` (the
//! hairline that splits areas).

use super::RADIUS_LG;
use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window, div,
    prelude::*, px,
};
use gpui_component::{ActiveTheme as _, v_flex};

#[derive(IntoElement, Default)]
pub struct Card {
    style: StyleRefinement,
    children: Vec<AnyElement>,
    /// The popover fill (a card floating over the page) instead of the muted one (a card on the page).
    raised: bool,
    /// No padding: rows inside draw their own (a list of [`super::group_row`]s).
    flush: bool,
}

impl Card {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn raised(mut self) -> Self {
        self.raised = true;
        self
    }

    pub fn flush(mut self) -> Self {
        self.flush = true;
        self
    }
}

impl Styled for Card {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = v_flex()
            .rounded(RADIUS_LG)
            .border_1()
            .border_color(theme.border)
            .bg(if self.raised {
                theme.popover
            } else {
                theme.muted
            })
            .overflow_hidden()
            .when(!self.flush, |d| d.p_4().gap_3())
            .when(self.raised, |d| d.shadow_md());
        el.style().refine(&self.style);
        el.children(self.children)
    }
}

/// A hairline: horizontal across its parent by default, `vertical()` for a 1 px column in a row.
#[derive(IntoElement, Default)]
pub struct Divider {
    vertical: bool,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self::default()
    }

    pub fn vertical() -> Self {
        Self { vertical: true }
    }
}

impl RenderOnce for Divider {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let line = div().flex_shrink_0().bg(cx.theme().border);
        if self.vertical {
            line.w(px(1.)).h_full()
        } else {
            line.h(px(1.)).w_full()
        }
    }
}
