//! The loading spinner: our loader glyph turning once every 0.8 s, eased in and out. Under reduce motion GPUI holds it
//! still and asks for no frames.

use super::{Icon, SkyeIcon};
use gpui::{
    Animation, AnimationExt as _, App, Hsla, IntoElement, ParentElement as _, Pixels, RenderOnce,
    Styled as _, Transformation, Window, div, ease_in_out, percentage, prelude::FluentBuilder as _,
    px,
};
use std::time::Duration;

/// The glyph the spinner turns.
pub(super) const GLYPH: SkyeIcon = SkyeIcon::Loader;

#[derive(IntoElement)]
pub struct Spinner {
    size: Pixels,
    color: Option<Hsla>,
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spinner {
    pub fn new() -> Self {
        Self {
            size: px(16.),
            color: None,
        }
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Spinner {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().child(
            Icon::new(GLYPH)
                .size(self.size)
                .when_some(self.color, |icon, color| icon.text_color(color))
                .with_animation(
                    "spinner",
                    Animation::new(Duration::from_millis(800))
                        .repeat()
                        .with_easing(ease_in_out),
                    |icon, delta| icon.transform(Transformation::rotate(percentage(delta))),
                ),
        )
    }
}
