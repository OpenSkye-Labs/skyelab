//! T3's input (`ui/input.tsx`): a single-line field at the control sizes ([`Size`]), the input-colored
//! hairline turning `ring` while focused, canvas fill (input at 32% in dark themes). The editing is [`TextInput`].
//! A field that sits inside a chrome of its own (the palette, a search strip) is [`TextField::bare`].

use super::{DISABLED_OPACITY, Icon, Size, SkyeIcon, TextInput};
use crate::ui::ActiveTheme as _;
use crate::ui::h_flex;
use gpui::{
    AnyElement, App, Div, Entity, Focusable as _, IntoElement, ParentElement as _, RenderOnce,
    Role, SharedString, Styled as _, Window, div, prelude::*, px, rems,
};

#[derive(Clone, Copy, PartialEq)]
enum Chrome {
    Field,
    Bare,
    BareLarge,
}

#[derive(IntoElement)]
pub struct TextField {
    state: Entity<TextInput>,
    size: Size,
    chrome: Chrome,
    prefix: Option<AnyElement>,
    suffix: Option<AnyElement>,
    aria_label: Option<SharedString>,
    aria_id: Option<SharedString>,
    cleanable: bool,
    disabled: bool,
    invalid: bool,
}

impl TextField {
    pub fn new(state: &Entity<TextInput>) -> Self {
        Self {
            state: state.clone(),
            size: Size::Md,
            chrome: Chrome::Field,
            prefix: None,
            suffix: None,
            aria_label: None,
            aria_id: None,
            cleanable: false,
            disabled: false,
            invalid: false,
        }
    }

    /// No border, fill or control height: the box around it draws those (a search strip).
    pub fn bare(mut self) -> Self {
        self.chrome = Chrome::Bare;
        self
    }

    /// [`Self::bare`] at the palette's size: body text in a tall row, no padding.
    pub fn bare_large(mut self) -> Self {
        self.chrome = Chrome::BareLarge;
        self
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
        let (label, id, disabled) = (self.aria_label, self.aria_id, self.disabled);
        let clear = self.cleanable && !disabled && !self.state.read(cx).value().is_empty();
        self.state.update(cx, |state, _| {
            state.frame.label = label;
            state.frame.accessibility_id = id;
            state.frame.disabled = disabled;
        });
        let theme = cx.theme();
        let dim = |el: Div| el.when(disabled, |d| d.opacity(0.5));
        let has_suffix = clear || self.suffix.is_some();
        let state = self.state.clone();
        let line = h_flex()
            .size_full()
            .line_height(rems(1.25))
            .text_color(theme.foreground)
            .map(|line| match self.chrome {
                Chrome::Field => line.gap(px(6.)).text_size(self.size.text()),
                Chrome::Bare => line.gap_1().h_6().px(px(8.)).py(px(2.)).text_sm(),
                Chrome::BareLarge => line.gap_2().h_11().text_base(),
            })
            // A press on a disabled field does nothing, for the rows around it too.
            .when(disabled, |line| {
                line.capture_any_mouse_down(|_, _, cx| cx.stop_propagation())
            })
            .children(self.prefix.map(|p| dim(div()).child(p)))
            .child(self.state.clone())
            .when(has_suffix, |line| {
                let suffix = dim(h_flex()).id("suffix").gap(px(6.)).cursor_default();
                line.pr(px(10.)).child(
                    suffix
                        .when(clear, |s| s.child(clear_button(state, cx)))
                        .children(self.suffix),
                )
            });
        if self.chrome != Chrome::Field {
            return line.into_any_element();
        }
        let size = self.size;
        let border = if self.invalid {
            theme.danger
        } else if focused {
            theme.ring
        } else {
            theme.input
        };
        let field = h_flex()
            .w_full()
            .min_w_0()
            .h(size.height())
            .px(size.pad_x() - px(1.))
            .rounded(size.radius())
            .border_1()
            .border_color(border)
            .bg(if theme.is_dark() {
                theme.input.opacity(0.32)
            } else {
                theme.background
            })
            .text_size(size.text())
            .when(disabled, |d| d.opacity(DISABLED_OPACITY))
            .child(line);
        crate::settings::scroll::track_focused(focused, field).into_any_element()
    }
}

/// The × of a `cleanable` field: an edit (it can be undone and emits `Change`), and the focus stays in the field.
fn clear_button(state: Entity<TextInput>, cx: &App) -> impl IntoElement {
    let ink = cx.theme().foreground;
    div()
        .id("clean")
        .role(Role::Button)
        .aria_label("Clear")
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .text_color(ink.opacity(0.9))
        .hover(move |s| s.text_color(ink))
        .active(move |s| s.text_color(ink.opacity(0.7)))
        .child(Icon::new(SkyeIcon::Clear).size_3())
        .on_click(move |_, window, cx| {
            state.update(cx, |state, cx| {
                state.accessibility_set_value("", window, cx);
                state.focus(window, cx);
            });
        })
}
