//! T3's input (`ui/input.tsx`): a single-line field at the control sizes ([`Size`]), the input-colored
//! hairline turning `ring` while focused, canvas fill (input at 32% in dark themes). The editing itself is
//! gpui-component's `Input` (text input is not built into GPUI, docs/gpui/elements.md), drawn here without its own
//! chrome. Inputs that sit inside a chrome of their own (the composer, a search strip) keep the bare `Input` with
//! `appearance(false)`.

use super::Size;
use gpui::{
    AnyElement, App, Entity, Focusable as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, prelude::*,
};
use gpui_component::{
    ActiveTheme as _, h_flex,
    input::{Input, InputState},
};

#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    size: Size,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    aria_label: Option<SharedString>,
    aria_id: Option<SharedString>,
    cleanable: bool,
    disabled: bool,
    invalid: bool,
}

impl TextField {
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            size: Size::Md,
            prefix: None,
            suffix: None,
            aria_label: None,
            aria_id: None,
            cleanable: false,
            disabled: false,
            invalid: false,
        }
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn xsmall(self) -> Self {
        self.with_size(Size::Xs)
    }
    pub fn small(self) -> Self {
        self.with_size(Size::Sm)
    }

    /// Something before the text (a search icon).
    pub fn prefix(mut self, el: impl IntoElement) -> Self {
        self.prefix = Some(el.into_any_element());
        self
    }

    /// Something after the text (a unit, a button).
    pub fn suffix(mut self, el: impl IntoElement) -> Self {
        self.suffix = Some(el.into_any_element());
        self
    }

    /// Accessible name exposed for screen readers and `agent-desktop`.
    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.aria_id = Some(id.into());
        self
    }

    /// A × that clears the field while it has text.
    pub fn cleanable(mut self, cleanable: bool) -> Self {
        self.cleanable = cleanable;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks the field as containing an invalid value (border switches to `theme.danger`).
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        let theme = cx.theme();
        let size = self.size;
        let border = if self.invalid {
            theme.danger
        } else if focused {
            theme.ring
        } else {
            theme.input
        };
        let input = Input::new(&self.state)
            .appearance(false)
            .cleanable(self.cleanable)
            .disabled(self.disabled)
            .when_some(self.aria_label, |i, l| i.aria_label(l))
            .when_some(self.aria_id, |i, id| i.accessibility_id(id))
            .when_some(self.prefix, |i, p| i.prefix(p))
            .when_some(self.suffix, |i, s| i.suffix(s))
            .px_0()
            .py_0()
            .h_full()
            // The Input sets its own text size; ours goes on top, so `Size::text` sizes the field too.
            .text_size(size.text());
        h_flex()
            .w_full()
            .min_w_0()
            .h(size.height())
            .px(size.pad_x() - gpui::px(1.))
            .rounded(size.radius())
            .border_1()
            .border_color(border)
            .bg(if theme.is_dark() {
                theme.input.opacity(0.32)
            } else {
                theme.background
            })
            .text_size(size.text())
            .when(self.disabled, |d| d.opacity(super::DISABLED_OPACITY))
            .child(input)
    }
}
