//! A status bar: items pinned to the left and right ends, and a center whose children sit centered between both ends,
//! at the end after a left alone, or at the start otherwise.

use super::{ActiveTheme as _, h_flex};
use gpui::{
    AnyElement, App, IntoElement, ParentElement, RenderOnce, StyleRefinement, Styled, Window,
    prelude::FluentBuilder as _,
};

#[derive(IntoElement, Default)]
pub struct StatusBar {
    style: StyleRefinement,
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
    center: Vec<AnyElement>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn left(mut self, child: impl IntoElement) -> Self {
        self.left.push(child.into_any_element());
        self
    }

    pub fn right(mut self, child: impl IntoElement) -> Self {
        self.right.push(child.into_any_element());
        self
    }
}

/// `child` / `children` fill the center.
impl ParentElement for StatusBar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.center.extend(elements);
    }
}

impl Styled for StatusBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (has_left, has_right) = (!self.left.is_empty(), !self.right.is_empty());
        let region = || h_flex().overflow_hidden().gap_2();
        let mut bar = h_flex()
            .gap_2()
            .py_1()
            .px_2()
            .border_t_1()
            .border_color(theme.status_bar_border)
            .bg(theme.status_bar)
            .text_xs()
            .text_color(theme.muted_foreground);
        gpui::Refineable::refine(bar.style(), &self.style);
        bar.when(has_left, |bar| bar.child(region().children(self.left)))
            .child(
                region()
                    .flex_1()
                    .when(has_left && has_right, |r| r.justify_center())
                    .when(has_left && !has_right, |r| r.justify_end())
                    .children(self.center),
            )
            .when(has_right, |bar| bar.child(region().children(self.right)))
    }
}
