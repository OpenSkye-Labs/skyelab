//! On/off controls after T3's (`ui/switch.tsx`, `ui/checkbox.tsx`): a 30 × 18 switch with a 16 px thumb, primary
//! when on; a 16 px checkbox, radius 4, primary fill with a check when on. Stateless: the caller owns the value and
//! gets the flipped one in `on_click`.
//!
//! The row is gpui-base's headless button, so it is a tab stop that Space and Enter flip, and a disabled one swallows
//! the press. An enabled one's click still bubbles: a row that is itself clickable takes the handler, and the control
//! inside it is only its picture (docs/gpui/elements.md).

use super::{DISABLED_OPACITY, META, RADIUS_SM, SkyeIcon};
use gpui::{
    App, ClickEvent, ElementId, FontWeight, IntoElement, ParentElement as _, RenderOnce,
    SharedString, Styled as _, Window, accesskit, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon};
use std::rc::Rc;

type OnToggle = Rc<dyn Fn(&bool, &mut Window, &mut App)>;

/// What a switch and a checkbox share: id, value, label, disabled, size, handler.
struct Toggle {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    aria: Option<SharedString>,
    aria_id: Option<SharedString>,
    disabled: bool,
    small: bool,
    decorative: bool,
    on_click: Option<OnToggle>,
}

impl Toggle {
    fn new(id: ElementId) -> Self {
        Self {
            id,
            checked: false,
            label: None,
            aria: None,
            aria_id: None,
            disabled: false,
            small: false,
            decorative: false,
            on_click: None,
        }
    }

    /// The row: the control, then its label, one hit target.
    fn render(
        self,
        role: accesskit::Role,
        control: impl IntoElement,
        cx: &App,
    ) -> gpui::AnyElement {
        let checked = self.checked;
        let interactive = !self.disabled;
        let theme = cx.theme();
        let ring = theme.ring;
        if self.decorative {
            return gpui_component::h_flex()
                .flex_shrink_0()
                .justify_start()
                .gap_2()
                .p(px(1.))
                .rounded(RADIUS_SM)
                .border_1()
                .border_color(gpui::transparent_black())
                .when(self.disabled, |d| d.opacity(DISABLED_OPACITY))
                .child(control)
                .when_some(self.label, |d, l| {
                    d.child(
                        div()
                            .text_size(if self.small { META } else { super::BODY })
                            .text_color(theme.foreground)
                            .child(l),
                    )
                })
                .into_any_element();
        }
        let a11y_label = self.aria.or_else(|| self.label.clone());
        debug_assert!(
            a11y_label.as_deref().is_some_and(|s| !s.trim().is_empty()),
            "ui::Switch/Checkbox({:?}) must have a non-empty label or accessibility_label, or use .decorative() inside a clickable row (docs/gpui/accessibility.md)",
            self.id
        );
        let id = self.id.clone();
        let button = super::a11y_scroll::scrolls_into_view(gpui_base::Button::new(self.id))
            .role(role)
            .disabled(self.disabled)
            .flex_shrink_0()
            .justify_start()
            .gap_2()
            .p(px(1.))
            .rounded(RADIUS_SM)
            .border_1()
            .border_color(gpui::transparent_black())
            .aria_toggled(checked.into())
            .when_some(a11y_label, |d, l| d.accessibility_label(l))
            .when_some(self.aria_id, |d, id| d.accessibility_id(id))
            .when(interactive, |d| {
                d.cursor_pointer()
                    // Like a button press: the field being typed in keeps focus.
                    .on_any_mouse_down(|_, window, _| window.prevent_default())
            })
            .focus_visible(move |s| s.border_color(ring))
            .when(self.disabled, |d| d.opacity(DISABLED_OPACITY))
            .child(control)
            .when_some(self.label, |d, l| {
                d.child(
                    div()
                        .text_size(if self.small { META } else { super::BODY })
                        .text_color(theme.foreground)
                        .child(l),
                )
            })
            .when_some(self.on_click.filter(|_| interactive), |d, f| {
                d.on_click(move |_: &ClickEvent, window, cx| f(&!checked, window, cx))
            });
        crate::settings::scroll::track_button(id, button).into_any_element()
    }
}

macro_rules! toggle_builders {
    ($ty:ident) => {
        impl $ty {
            pub fn new(id: impl Into<ElementId>) -> Self {
                Self(Toggle::new(id.into()))
            }
            pub fn checked(mut self, checked: bool) -> Self {
                self.0.checked = checked;
                self
            }
            pub fn label(mut self, label: impl Into<SharedString>) -> Self {
                self.0.label = Some(label.into());
                self
            }
            /// The name a screen reader announces when there is no visible label.
            pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
                self.0.aria = Some(label.into());
                self
            }
            pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
                self.0.aria_id = Some(id.into());
                self
            }
            pub fn disabled(mut self, disabled: bool) -> Self {
                self.0.disabled = disabled;
                self
            }
            /// The compact size: smaller control, 12 px label.
            pub fn xsmall(mut self) -> Self {
                self.0.small = true;
                self
            }
            /// Gets the value the control flips to.
            pub fn on_click(mut self, f: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
                self.0.on_click = Some(Rc::new(f));
                self
            }
        }
    };
}

#[derive(IntoElement)]
pub struct Switch(Toggle);
toggle_builders!(Switch);

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (thumb, pad) = if self.0.small {
            (px(12.), px(1.))
        } else {
            (px(16.), px(1.))
        };
        let on = self.0.checked;
        let track_w = thumb * 2. - px(2.);
        let travel = track_w - pad * 2. - thumb;
        // Retargetable spring for the thumb: 0 idle frames at rest, instant under Reduce Motion.
        let offset = gpui_base::spring(
            self.0.id.clone(),
            if on { travel } else { px(0.) },
            super::motion::SPRING_PX,
            window,
            cx,
        );
        let theme = cx.theme();
        let track = div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .w(track_w)
            .h(thumb + pad * 2.)
            .p(pad)
            .rounded_full()
            .bg(if on { theme.primary } else { theme.input })
            .child(
                div()
                    .ml(offset)
                    .size(thumb)
                    .rounded_full()
                    .bg(if on {
                        theme.primary_foreground
                    } else {
                        theme.background
                    })
                    .shadow_xs(),
            );
        self.0.render(accesskit::Role::Switch, track, cx)
    }
}

#[derive(IntoElement)]
pub struct Checkbox(Toggle);
toggle_builders!(Checkbox);

impl Checkbox {
    /// Visual-only indicator inside an enclosing clickable row (the row owns role, name, and click).
    pub fn decorative(mut self) -> Self {
        self.0.decorative = true;
        self
    }
}

impl RenderOnce for Checkbox {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let size = if self.0.small { px(14.) } else { px(16.) };
        let on = self.0.checked;
        let bx = div()
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .size(size)
            .rounded(RADIUS_SM)
            .border_1()
            .border_color(if on { theme.primary } else { theme.input })
            .bg(if on { theme.primary } else { theme.background })
            .font_weight(FontWeight::BOLD)
            .when(on, |d| {
                d.child(
                    Icon::new(SkyeIcon::Check)
                        .size(size - px(4.))
                        .text_color(theme.primary_foreground),
                )
            });
        self.0.render(accesskit::Role::CheckBox, bx, cx)
    }
}
