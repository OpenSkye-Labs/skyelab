//! The loading spinner. Under reduce motion GPUI holds it still and asks for no frames.

use super::{Icon, SkyeIcon};
use gpui::{
    AbsoluteLength, Animation, AnimationExt as _, App, Hsla, IntoElement, ParentElement as _,
    RenderOnce, Styled as _, Transformation, Window, div, ease_in_out, percentage,
    prelude::FluentBuilder as _, px,
};
use std::time::Duration;

pub(super) const GLYPH: SkyeIcon = SkyeIcon::Loader;

#[derive(IntoElement)]
pub struct Spinner {
    size: AbsoluteLength,
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
            size: px(16.).into(),
            color: None,
        }
    }

    pub fn size(mut self, size: impl Into<AbsoluteLength>) -> Self {
        self.size = size.into();
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
