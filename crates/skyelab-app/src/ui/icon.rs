//! An icon from our set ([`SkyeIcon`]), drawn at the text size and in the text color around it unless told otherwise.

use super::SkyeIcon;
use gpui::{
    App, Hsla, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled, Transformation,
    Window, svg,
};

#[derive(IntoElement, Clone)]
pub struct Icon {
    style: StyleRefinement,
    path: SharedString,
    color: Option<Hsla>,
    transformation: Option<Transformation>,
}

impl Icon {
    pub fn new(icon: impl Into<Icon>) -> Self {
        icon.into()
    }

    /// Rotates or scales the drawn glyph (the spinner turns it) without changing its layout box.
    pub fn transform(mut self, transformation: Transformation) -> Self {
        self.transformation = Some(transformation);
        self
    }
}

impl From<SkyeIcon> for Icon {
    fn from(icon: SkyeIcon) -> Self {
        Self {
            style: StyleRefinement::default(),
            path: icon.path(),
            color: None,
            transformation: None,
        }
    }
}

impl Styled for Icon {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }

    fn text_color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let text = window.text_style();
        let sized = self.style.size.width.is_some() || self.style.size.height.is_some();
        let mut el = svg();
        *el.style() = self.style;
        let el = el
            .flex_shrink_0()
            .text_color(self.color.unwrap_or(text.color))
            .path(self.path);
        let el = match self.transformation {
            Some(t) => el.with_transformation(t),
            None => el,
        };
        if sized {
            el
        } else {
            el.size(text.font_size.to_pixels(window.rem_size()))
        }
    }
}
