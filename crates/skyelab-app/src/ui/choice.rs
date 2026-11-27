//! Pick-one controls. `SegmentedControl` is T3's toggle group (`ui/toggle-group.tsx`): a well in the input color,
//! the chosen segment lifted out of it. `Tabs` is the underlined row used over a page's
//! sections. Both are stateless: the caller owns the selected index and gets the clicked one in `on_click`.

use super::{META, Size, SkyeIcon};
use gpui::{
    App, ElementId, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, accesskit, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, Icon, h_flex};
use std::rc::Rc;

type OnPick = Rc<dyn Fn(&usize, &mut Window, &mut App)>;

struct Item {
    label: Option<SharedString>,
    icon: Option<SkyeIcon>,
}

struct Choice {
    id: ElementId,
    aria_label: Option<SharedString>,
    aria_id: Option<SharedString>,
    items: Vec<Item>,
    selected: Option<usize>,
    size: Size,
    on_click: Option<OnPick>,
}

macro_rules! choice_builders {
    ($ty:ident) => {
        impl $ty {
            pub fn new(id: impl Into<ElementId>) -> Self {
                Self(Choice {
                    id: id.into(),
                    aria_label: None,
                    aria_id: None,
                    items: Vec::new(),
                    selected: None,
                    size: Size::Xs,
                    on_click: None,
                })
            }
            pub fn accessibility_label(mut self, label: impl Into<SharedString>) -> Self {
                self.0.aria_label = Some(label.into());
                self
            }
            pub fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
                self.0.aria_id = Some(id.into());
                self
            }
            pub fn item(mut self, label: impl Into<SharedString>) -> Self {
                self.0.items.push(Item {
                    label: Some(label.into()),
                    icon: None,
                });
                self
            }
            /// An item with an icon before its label.
            pub fn icon_item(mut self, icon: SkyeIcon, label: impl Into<SharedString>) -> Self {
                let label: SharedString = label.into();
                self.0.items.push(Item {
                    label: (!label.is_empty()).then_some(label),
                    icon: Some(icon),
                });
                self
            }
            pub fn items<S: Into<SharedString>>(
                mut self,
                labels: impl IntoIterator<Item = S>,
            ) -> Self {
                for l in labels {
                    self = self.item(l);
                }
                self
            }
            pub fn selected(mut self, ix: Option<usize>) -> Self {
                self.0.selected = ix;
                self
            }
            pub fn with_size(mut self, size: Size) -> Self {
                self.0.size = size;
                self
            }
            /// Gets the clicked item's index (also when it is already the selected one).
            pub fn on_click(mut self, f: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
                self.0.on_click = Some(Rc::new(f));
                self
            }
        }
    };
}

#[derive(IntoElement)]
pub struct SegmentedControl(Choice);
choice_builders!(SegmentedControl);

impl RenderOnce for SegmentedControl {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let c = self.0;
        let h = c.size.height() - px(4.);
        // Light: T3's white lift alone barely shows on a white dialog, so the selected item also gets a hairline.
        let (lift, hover, ring, edge) = if theme.is_dark() {
            (
                theme.input.opacity(0.72),
                theme.input.opacity(0.32),
                theme.ring,
                gpui::transparent_black(),
            )
        } else {
            (
                theme.background,
                theme.background.opacity(0.55),
                theme.ring,
                theme.input,
            )
        };
        h_flex()
            .id(c.id)
            .role(accesskit::Role::RadioGroup)
            .when_some(c.aria_label, |d, l| d.aria_label(l))
            .when_some(c.aria_id, |d, id| d.accessibility_id(id))
            .flex_shrink_0()
            .gap_0p5()
            .p_0p5()
            .rounded(c.size.radius())
            .bg(theme.input.opacity(0.4))
            .children(c.items.into_iter().enumerate().map(|(ix, item)| {
                let on = c.selected == Some(ix);
                let f = c.on_click.clone();
                let a11y_label = item.label.clone();
                debug_assert!(
                    a11y_label.as_deref().is_some_and(|s| !s.trim().is_empty()),
                    "ui::SegmentedControl item {ix} must have a non-empty label (docs/gpui/accessibility.md)"
                );
                // gpui-base's headless button: a tab stop that Space and Enter pick, a press that keeps focus put.
                let button = super::a11y_scroll::scrolls_into_view(gpui_base::Button::new(ix))
                    .selected(on)
                    .on_any_mouse_down(|_, window, _| window.prevent_default())
                    .role(accesskit::Role::RadioButton)
                    .when_some(a11y_label, |d, l| d.accessibility_label(l))
                    .aria_toggled(on.into())
                    .h(h)
                    .gap_1()
                    .px(c.size.pad_x() - px(2.))
                    .justify_center()
                    .rounded(c.size.radius() - px(2.))
                    .border_1()
                    .border_color(gpui::transparent_black())
                    .focus_visible(move |s| s.border_color(ring))
                    .text_size(c.size.text())
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(lift)
                                .border_color(edge)
                                .text_color(theme.foreground)
                                .shadow_xs()
                        } else {
                            d.text_color(theme.muted_foreground)
                                .hover(move |s| s.bg(hover))
                        }
                    })
                    .when_some(item.icon, |d, i| d.child(Icon::new(i).size(c.size.icon())))
                    .when_some(item.label, |d, l| d.child(l))
                    .when_some(f, |d, f| d.on_click(move |_, w, cx| f(&ix, w, cx)));
                crate::settings::scroll::track_button(ix, button)
            }))
    }
}

#[derive(IntoElement)]
pub struct Tabs(Choice);
choice_builders!(Tabs);

impl RenderOnce for Tabs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let c = self.0;
        h_flex()
            .id(c.id)
            .role(accesskit::Role::TabList)
            .when_some(c.aria_label, |d, l| d.aria_label(l))
            .when_some(c.aria_id, |d, id| d.accessibility_id(id))
            .gap_4()
            .border_b_1()
            .border_color(theme.border)
            .children(c.items.into_iter().enumerate().map(|(ix, item)| {
                let on = c.selected == Some(ix);
                let f = c.on_click.clone();
                let (fg, ring) = (theme.foreground, theme.ring);
                let a11y_label = item.label.clone();
                debug_assert!(
                    a11y_label.as_deref().is_some_and(|s| !s.trim().is_empty()),
                    "ui::Tabs item {ix} must have a non-empty label (docs/gpui/accessibility.md)"
                );
                // gpui-base's headless button: a tab stop that Space and Enter pick, a press that keeps focus put.
                let button = super::a11y_scroll::scrolls_into_view(gpui_base::Button::new(ix))
                    .selected(on)
                    .on_any_mouse_down(|_, window, _| window.prevent_default())
                    .role(accesskit::Role::Tab)
                    .when_some(a11y_label, |d, l| d.accessibility_label(l))
                    .aria_selected(on)
                    .h(c.size.height() + px(4.))
                    .gap_1p5()
                    // The underline sits on the row's hairline.
                    .mb(px(-1.))
                    .border_b_2()
                    .border_color(if on {
                        theme.foreground
                    } else {
                        gpui::transparent_black()
                    })
                    .focus_visible(move |s| s.border_color(ring).text_color(fg))
                    .text_size(if c.size == Size::Xs {
                        META
                    } else {
                        super::BODY
                    })
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .text_color(if on {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .when(!on, |d| d.hover(move |s| s.text_color(fg)))
                    .when_some(item.icon, |d, i| d.child(Icon::new(i).size(c.size.icon())))
                    .when_some(item.label, |d, l| d.child(div().child(l)))
                    .when_some(f, |d, f| d.on_click(move |_, w, cx| f(&ix, w, cx)));
                crate::settings::scroll::track_button(ix, button)
            }))
    }
}
