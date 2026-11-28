//! SkyeLab's button, after T3's (`ui/button.tsx`): sized by [`Size`], colored by [`Variant`] through [`colors`].
//! Built on gpui-base's headless button (role, focus, Enter/Space, the disabled click guard). Stateless: a `RenderOnce`
//! builder, no entity, hover and press drawn by GPUI's own styles. `.dropdown_menu(..)` opens a
//! [`super::PopupMenu`] from it.

use super::{DISABLED_OPACITY, Kbd, Size, SkyeIcon, Tooltip};
use crate::ui::Icon;
use crate::ui::{ActiveTheme as _, Theme};
use gpui::{
    Action, AnyElement, App, ClickEvent, ElementId, FontWeight, Hsla, Interactivity, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, Window, div,
    prelude::*, px,
};
use gpui_component::Selectable;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// The one main action of a surface: Send, Save, Create.
    Primary,
    /// A quieter filled action beside a primary one.
    Secondary,
    /// Toolbar and row actions: no box until hovered.
    Ghost,
    /// A standalone neutral action: Cancel, Choose…, Log in.
    #[default]
    Outline,
    /// Destroys or ends something: Delete, Force stop, Discard.
    Danger,
    /// Asks you to look again before going on (T3's `warning-outline`): Re-confirm.
    Warning,
}

/// One variant's colors over theme fields. Geometry comes from [`Size`]; this is where a variant's colors change.
pub(crate) struct Colors {
    pub bg: Hsla,
    pub hover: Hsla,
    pub active: Hsla,
    pub fg: Hsla,
    pub icon: Hsla,
    pub border: Hsla,
}

pub(crate) fn colors(variant: Variant, selected: bool, theme: &Theme) -> Colors {
    let clear = gpui::transparent_black();
    let c = match variant {
        Variant::Primary => Colors {
            bg: theme.primary,
            hover: theme.primary_hover,
            active: theme.primary_active,
            fg: theme.primary_foreground,
            icon: theme.primary_foreground,
            border: theme.primary,
        },
        Variant::Secondary => Colors {
            bg: theme.secondary,
            hover: theme.secondary_hover,
            active: theme.secondary_active,
            fg: theme.secondary_foreground,
            icon: theme.secondary_foreground,
            border: clear,
        },
        Variant::Ghost => Colors {
            bg: clear,
            hover: theme.accent,
            active: theme.accent,
            fg: theme.foreground,
            icon: theme.muted_foreground,
            border: clear,
        },
        Variant::Outline => Colors {
            bg: if theme.is_dark() {
                theme.input.opacity(0.32)
            } else {
                theme.popover
            },
            hover: if theme.is_dark() {
                theme.input.opacity(0.64)
            } else {
                theme.accent.opacity(0.5)
            },
            active: theme.accent,
            fg: theme.foreground,
            icon: theme.muted_foreground,
            border: theme.input,
        },
        Variant::Danger => Colors {
            bg: theme.danger,
            hover: theme.danger_hover,
            active: theme.danger_active,
            fg: theme.danger_foreground,
            icon: theme.danger_foreground,
            border: theme.danger,
        },
        Variant::Warning => Colors {
            bg: theme
                .warning
                .opacity(if theme.is_dark() { 0.16 } else { 0.08 }),
            hover: theme
                .warning
                .opacity(if theme.is_dark() { 0.24 } else { 0.16 }),
            active: theme.warning.opacity(0.24),
            fg: super::ink(theme, theme.warning),
            icon: super::ink(theme, theme.warning),
            border: theme.warning.opacity(0.32),
        },
    };
    // Selected (a toggle that is on, a trigger whose menu is open): the hover fill held, like T3's `data-pressed`.
    if selected {
        Colors {
            bg: c.hover,
            icon: c.fg,
            ..c
        }
    } else {
        c
    }
}

type OnClick = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type ActionRef = (Rc<dyn Action>, Option<SharedString>);

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    base: gpui_base::Button,
    variant: Variant,
    size: Size,
    label: Option<SharedString>,
    aria_label: Option<SharedString>,
    aria_id: Option<SharedString>,
    aria_description: Option<SharedString>,
    aria_expanded: Option<bool>,
    aria_selected: Option<bool>,
    role: Option<gpui::Role>,
    icon: Option<Icon>,
    icon_end: Option<Icon>,
    children: Vec<AnyElement>,
    disabled: bool,
    selected: bool,
    loading: bool,
    /// The loading spinner turns; off, it shows one still frame.
    spin: bool,
    caret: bool,
    square: bool,
    toggled: Option<bool>,
    tooltip: Option<(SharedString, Option<ActionRef>)>,
    menu_open: Option<bool>,
    key_hint: Option<ActionRef>,
    on_click: Option<OnClick>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>) -> Self {
        let id = id.into();
        Self {
            base: gpui_base::Button::new(id.clone()),
            id,
            variant: Variant::default(),
            size: Size::default(),
            label: None,
            aria_label: None,
            aria_id: None,
            aria_description: None,
            aria_expanded: None,
            aria_selected: None,
            role: None,
            icon: None,
            icon_end: None,
            children: Vec::new(),
            disabled: false,
            selected: false,
            loading: false,
            spin: true,
            caret: false,
            square: false,
            toggled: None,
            tooltip: None,
            menu_open: None,
            key_hint: None,
            on_click: None,
        }
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
    pub fn primary(self) -> Self {
        self.variant(Variant::Primary)
    }
    pub fn secondary(self) -> Self {
        self.variant(Variant::Secondary)
    }
    pub fn ghost(self) -> Self {
        self.variant(Variant::Ghost)
    }
    pub fn outline(self) -> Self {
        self.variant(Variant::Outline)
    }
    pub fn danger(self) -> Self {
        self.variant(Variant::Danger)
    }
    pub fn warning(self) -> Self {
        self.variant(Variant::Warning)
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
    pub fn medium(self) -> Self {
        self.with_size(Size::Md)
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
        self.aria_label = Some(label.into());
        self
    }

    pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.aria_id = Some(id.into());
        self
    }

    pub fn accessibility_description(mut self, description: impl Into<SharedString>) -> Self {
        self.aria_description = Some(description.into());
        self
    }

    pub fn accessibility_expanded(mut self, expanded: bool) -> Self {
        self.aria_expanded = Some(expanded);
        self
    }

    pub fn accessibility_selected(mut self, selected: bool) -> Self {
        self.aria_selected = Some(selected);
        self
    }

    pub fn role(mut self, role: gpui::Role) -> Self {
        self.role = Some(role);
        self
    }

    /// The leading icon.
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// An icon after the label (an external-link arrow, a chevron).
    pub fn icon_end(mut self, icon: impl Into<Icon>) -> Self {
        self.icon_end = Some(icon.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Held down: a toggle that is on, or a trigger whose menu is open.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Announces the button as a toggle that is on or off (screen readers only; [`Self::selected`] paints it).
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = Some(on);
        self
    }

    /// Shows a spinner in place of the icon and swallows clicks until the work is done.
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    /// Whether the loading spinner turns. A still frame draws once; a turning one asks for a frame on every vsync.
    pub fn spin(mut self, spin: bool) -> Self {
        self.spin = spin;
        self
    }

    /// A trailing chevron: this button opens a menu.
    pub fn dropdown_caret(mut self, caret: bool) -> Self {
        self.caret = caret;
        self
    }

    /// Square (icon only), whatever its content; [`icon_button`] sets it.
    pub fn square(mut self) -> Self {
        self.square = true;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some((text.into(), None));
        self
    }

    /// A tooltip that also shows the key bound to `action` in `context`.
    pub fn tooltip_with_action(
        mut self,
        text: impl Into<SharedString>,
        action: &dyn Action,
        context: Option<&str>,
    ) -> Self {
        self.tooltip = Some((
            text.into(),
            Some((
                Rc::from(action.boxed_clone()),
                context.map(SharedString::new),
            )),
        ));
        self
    }

    /// The key bound to `action`, drawn inside the button after the label.
    pub fn key_hint(mut self, action: &dyn Action, context: Option<&str>) -> Self {
        self.key_hint = Some((
            Rc::from(action.boxed_clone()),
            context.map(SharedString::new),
        ));
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    fn icon_el(icon: Icon, size: Pixels, color: Hsla) -> Icon {
        icon.size(size).text_color(color)
    }
}

/// The icon button: ghost, square at its size's height, the icon centered. Give it a tooltip: the icon is its only
/// label.
pub fn icon_button(id: impl Into<ElementId>, icon: impl Into<Icon>) -> Button {
    Button::new(id).ghost().icon(icon).square()
}

impl Selectable for Button {
    fn selected(self, selected: bool) -> Self {
        Button::selected(self, selected)
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Button {
    pub(super) fn menu_open(mut self, open: bool) -> Self {
        self.menu_open = Some(open);
        let selected = self.selected || open;
        Button::selected(self, selected)
    }

    pub(super) fn element_id(&self) -> ElementId {
        self.id.clone()
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl InteractiveElement for Button {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.base.interactivity()
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let c = colors(self.variant, self.selected, theme);
        let size = self.size;
        let interactive = !self.disabled && !self.loading;
        let square = self.square
            || (self.label.is_none()
                && self.children.is_empty()
                && !self.caret
                && self.icon_end.is_none()
                && self.key_hint.is_none());
        let key = self
            .key_hint
            .as_ref()
            .and_then(|(a, ctx)| Kbd::for_action(a.as_ref(), ctx.as_deref(), window));
        let (hover, active) = (c.hover, c.active);
        // Primary and Danger fills match or clash with `theme.ring`, so their inset focus ring uses their high-contrast
        // foreground color while neutral variants use `theme.ring`.
        let focus_ring = match self.variant {
            Variant::Primary => theme.primary_foreground,
            Variant::Danger => theme.danger_foreground,
            _ => theme.ring,
        };

        let a11y_label = self
            .aria_label
            .or_else(|| self.label.clone())
            .or_else(|| self.tooltip.as_ref().map(|(t, _)| t.clone()));
        debug_assert!(
            a11y_label.as_deref().is_some_and(|s| !s.trim().is_empty()),
            "ui::Button({:?}) must have a non-empty label, tooltip, or accessibility_label (docs/gpui/accessibility.md)",
            self.id
        );
        let dropdown_click = (self.menu_open.is_some() && interactive && self.on_click.is_none())
            .then(|| {
                window
                    .use_keyed_state(self.id.clone(), cx, |_, cx| cx.focus_handle())
                    .read(cx)
                    .clone()
            });

        let mut base = super::a11y_scroll::scrolls_into_view(self.base);
        // The caller's own style (`.w_full()`, `.rounded(..)`) goes on last, so it wins over ours.
        let caller = std::mem::take(base.style());
        let mut base = base
            .flex_shrink_0()
            .h(size.height())
            .gap(size.gap())
            .rounded(size.radius())
            .border_1()
            .border_color(c.border)
            .bg(c.bg)
            .text_color(c.fg)
            .text_size(size.text())
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .overflow_hidden()
            .map(|b| {
                if square {
                    b.w(size.height())
                } else {
                    b.px(size.pad_x() - px(1.))
                }
            })
            .when(interactive, |b| b.cursor_pointer())
            .when(interactive && !self.selected, |b| {
                b.hover(move |s| s.bg(hover)).active(move |s| s.bg(active))
            })
            .focus_visible(move |s| s.border_2().border_color(focus_ring))
            .when(self.disabled, |b| b.opacity(DISABLED_OPACITY))
            .when(self.loading && !self.disabled, |b| b.opacity(0.8))
            .selected(self.selected)
            .disabled(self.disabled)
            .when_some(self.role, |b, r| b.role(r))
            // An icon-only button is named by its tooltip.
            .when_some(a11y_label, |b, l| b.accessibility_label(l))
            .when_some(self.aria_id, |b, id| b.accessibility_id(id))
            .when_some(self.aria_description, |b, d| b.aria_description(d))
            .when_some(self.aria_selected, |b, s| b.aria_selected(s))
            .when_some(self.aria_expanded.or(self.menu_open), |b, e| {
                b.aria_expanded(e)
            })
            .when_some(self.toggled, |b, on| b.aria_toggled(on.into()))
            .when_some(dropdown_click, |b, trigger_focus| {
                let dispatch_focus = trigger_focus.clone();
                b.track_focus(&trigger_focus).on_a11y_action(
                    gpui::accesskit::Action::Click,
                    move |_, window, cx| {
                        dispatch_focus.dispatch_action(&super::popover::Toggle, window, cx);
                    },
                )
            });
        base.style().refine(&caller);

        let lead = if self.loading && (!self.spin || cx.reduce_motion()) {
            Some(
                Self::icon_el(Icon::new(SkyeIcon::LoaderCircle), size.icon(), c.icon)
                    .into_any_element(),
            )
        } else if self.loading {
            Some(
                super::Spinner::new()
                    .size(size.icon())
                    .color(c.icon)
                    .into_any_element(),
            )
        } else {
            self.icon
                .map(|i| Self::icon_el(i, size.icon(), c.icon).into_any_element())
        };
        let fg = c.fg;
        let button = base
            .when_some(lead, |b, i| b.child(i))
            .when_some(self.label, |b, label| {
                b.child(
                    div()
                        .min_w_0()
                        .text_ellipsis()
                        .overflow_hidden()
                        .child(label),
                )
            })
            .children(self.children)
            .when_some(key, |b, k| b.child(k.bare()))
            .when_some(self.icon_end, |b, i| {
                b.child(Self::icon_el(i, size.icon(), c.icon))
            })
            .when(self.caret, |b| {
                b.child(Self::icon_el(
                    Icon::new(SkyeIcon::ChevronDown),
                    size.icon() - px(2.),
                    fg.opacity(0.75),
                ))
            })
            // A press never takes focus from the field you were typing in, nor starts a text selection.
            .when(!self.disabled, |b| {
                let loading = self.loading;
                b.on_any_mouse_down(move |_, window, cx| {
                    window.prevent_default();
                    if loading {
                        // A loading button is inert: its press reaches no menu trigger or row around it.
                        cx.stop_propagation();
                        return;
                    }
                    if cx.has_global::<gpui_base::GlobalState>() {
                        gpui_base::GlobalState::suppress_text_selection(cx);
                    }
                })
            })
            .map(|b| match (self.loading, self.on_click) {
                (true, _) => b.on_click(|_, _, cx| cx.stop_propagation()),
                (false, Some(f)) => b.on_click(move |e, w, cx| f(e, w, cx)),
                (false, None) => b,
            })
            .when_some(
                self.tooltip.filter(|_| !self.menu_open.unwrap_or(false)),
                |b, (text, action)| {
                    b.tooltip(move |window, cx| {
                        let t = Tooltip::new(text.clone());
                        match &action {
                            Some((a, ctx)) => t.action(a.as_ref(), ctx.as_deref()),
                            None => t,
                        }
                        .build(window, cx)
                    })
                },
            );
        crate::settings::scroll::track_button(self.id, button)
    }
}

#[cfg(test)]
#[path = "button_menu_open_test.rs"]
mod menu_open_test;
