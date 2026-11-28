use super::a11y_disabled::a11y_disabled;
use super::menu::{KEY_CONTEXT, Kind, POPUP_PRIORITY, PopupMenu};
use super::{ActiveTheme as _, Icon, Kbd, SkyeIcon, h_flex, v_flex};
use gpui::{
    Anchor, AnyElement, BoxShadow, Context, Edges, Focusable as _, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Rems, Render, Role,
    StatefulInteractiveElement as _, Styled as _, Toggled, Window, anchored, canvas, deferred, div,
    hsla, point, prelude::FluentBuilder as _, px, relative, rems,
};
use gpui_base::TestSupportExt as _;

const ROW_H: Rems = rems(1.625);

/// The popup surface: the theme's popover fill, a hairline ring and a small two-layer shadow.
pub(crate) fn popover_surface<E: gpui::Styled>(el: E, cx: &gpui::App) -> E {
    let theme = cx.theme();
    let ring = hsla(
        theme.foreground.h,
        theme.foreground.s,
        theme.foreground.l,
        0.1,
    );
    let ink = hsla(0., 0., 0., 0.1);
    el.bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow(vec![
            BoxShadow {
                color: ring,
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            },
            BoxShadow {
                color: ink,
                offset: point(px(0.), px(4.)),
                blur_radius: px(3.),
                spread_radius: px(-1.),
                inset: false,
            },
            BoxShadow {
                color: ink,
                offset: point(px(0.), px(2.)),
                blur_radius: px(2.),
                spread_radius: px(-2.),
                inset: false,
            },
        ])
        .rounded(theme.radius)
}

impl Render for PopupMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let count = self.items.len();
        let has_left_icon = self
            .items
            .iter()
            .any(|i| i.has_left_icon(self.checks_right));
        let rows: Vec<AnyElement> = (0..count)
            .filter(|&ix| !(ix + 1 == count && matches!(self.items[ix].kind, Kind::Separator)))
            .map(|ix| self.row(ix, has_left_icon, window, cx))
            .collect();
        let menu = v_flex()
            .id("popup-menu")
            .test_support()
            .role(Role::Menu)
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::dismiss))
            .on_mouse_down_out(cx.listener(|this, e: &gpui::MouseDownEvent, window, cx| {
                if !this.chain_contains(e.position, cx) {
                    this.dismiss(&super::menu::Cancel, window, cx);
                }
            }));
        popover_surface(menu, cx)
            .text_color(cx.theme().popover_foreground)
            .relative()
            .occlude()
            .child(
                v_flex()
                    .id("items")
                    .p_1()
                    .gap_y_0p5()
                    .min_w(rems(8.))
                    .when_some(self.min_w, |d, w| d.min_w(w))
                    .max_w(rems(31.25))
                    .children(rows),
            )
            .child(
                canvas(
                    move |bounds, _, cx| view.update(cx, |m, _| m.bounds = bounds),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
    }
}

impl PopupMenu {
    fn row(
        &self,
        ix: usize,
        has_left_icon: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let item = &self.items[ix];
        let theme = cx.theme();
        let selected = self.selected == Some(ix);
        let radius = theme.radius.min(px(8.));
        let pickable = item.pickable();
        let checked = (item.checked || item.checkable).then_some(item.checked);
        let left_check = !self.checks_right && item.checked;
        let right_check =
            (self.checks_right && item.checked).then(|| Icon::new(SkyeIcon::Check).size_3());
        let is_submenu = matches!(item.kind, Kind::Submenu(..));
        let mut row = h_flex()
            .id(ix)
            .when(!matches!(item.kind, Kind::Separator), |d| {
                d.role(Role::MenuItem)
            })
            .when_some(item.name(), |d, name| d.aria_label(name))
            .aria_selected(selected)
            .when_some(checked, |d, on| {
                d.aria_toggled(if on { Toggled::True } else { Toggled::False })
            })
            .gap_x_1()
            .px(rems(0.5))
            .text_sm()
            .text_color(theme.foreground)
            .relative()
            .justify_between()
            .rounded(radius)
            .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                if *hovered {
                    let sub = this.submenu_entity();
                    if this.selected != Some(ix)
                        && sub.is_some_and(|s| s.focus_handle(cx).contains_focused(window, cx))
                    {
                        this.focus.focus(window, cx);
                    }
                    this.select(Some(ix), cx);
                    if is_submenu {
                        this.open_submenu_at(ix, false, window, cx);
                    }
                } else if !is_submenu
                    && this.selected == Some(ix)
                    && !window.last_input_was_keyboard()
                {
                    this.select(None, cx);
                }
            }));
        if pickable {
            let (accent, accent_fg) = (theme.accent, theme.accent_foreground);
            row = row
                .hover(move |s| s.bg(accent).text_color(accent_fg))
                .when(selected, |d| d.bg(accent).text_color(accent_fg))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, e, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                    this.select(Some(ix), cx);
                    this.activate(ix, e, window, cx);
                }));
        } else {
            row = row.text_color(theme.muted_foreground);
        }
        let icon = |icon: Option<Icon>, check: bool| -> Option<AnyElement> {
            has_left_icon.then(|| match icon {
                Some(i) => i.size_3().into_any_element(),
                None if check => Icon::new(SkyeIcon::Check).size_3().into_any_element(),
                None => div().size_3().flex_shrink_0().into_any_element(),
            })
        };
        let disabled = !pickable && !matches!(item.kind, Kind::Separator);
        let row = match &item.kind {
            Kind::Separator => row
                .h_auto()
                .p_0()
                .my_0p5()
                .mx_neg_1()
                .border_b(px(2.))
                .border_color(theme.border),
            Kind::Label(label) => row.child(
                h_flex()
                    .gap_x_1()
                    .children(icon(None, false))
                    .child(div().flex_1().child(label.clone())),
            ),
            Kind::Element(_, render) => row.child(
                h_flex()
                    .flex_1()
                    .min_h(ROW_H)
                    .gap_x_1()
                    .children(icon(item.icon.clone(), left_check))
                    .child(render(window, cx))
                    .children(right_check.map(|i| i.ml(rems(0.75)))),
            ),
            Kind::Row(label) => {
                let key = item
                    .action
                    .as_ref()
                    .and_then(|a| self.key_for(a.as_ref(), window, cx));
                row.h(ROW_H)
                    .children(icon(item.icon.clone(), left_check))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .justify_between()
                            .child(label.clone())
                            .children(right_check)
                            .children(key),
                    )
            }
            Kind::Submenu(label, menu) => {
                let open = self.open_submenu == Some(ix);
                let left = self.bounds.size.width - rems(0.5).to_pixels(window.rem_size());
                row.items_start()
                    .child(
                        h_flex()
                            .min_h(ROW_H)
                            .size_full()
                            .gap_x_1()
                            .children(icon(item.icon.clone(), false))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .gap_2()
                                    .justify_between()
                                    .child(label.clone())
                                    .child(
                                        Icon::new(SkyeIcon::ChevronRight)
                                            .size_3()
                                            .text_color(theme.muted_foreground),
                                    ),
                            ),
                    )
                    .when(open, |d| {
                        d.child(
                            deferred(
                                anchored()
                                    .anchor(Anchor::TopLeft)
                                    .child(
                                        div()
                                            .id("submenu")
                                            .occlude()
                                            .top_neg_1()
                                            .left(left)
                                            .child(menu.clone()),
                                    )
                                    .snap_to_window_with_margin(Edges::all(px(4.))),
                            )
                            .with_priority(POPUP_PRIORITY + self.depth + 1),
                        )
                    })
            }
        };
        a11y_disabled(row, disabled)
            .test_support()
            .into_any_element()
    }

    fn key_for(
        &self,
        action: &dyn gpui::Action,
        window: &Window,
        cx: &gpui::App,
    ) -> Option<AnyElement> {
        let kbd = self
            .key_paths(cx)
            .iter()
            .find_map(|f| Kbd::for_action_in(action, f, window))
            .or_else(|| Kbd::for_action(action, Some(""), window))?;
        let theme = cx.theme();
        Some(
            div()
                .text_color(theme.muted_foreground)
                .min_w_5()
                .text_center()
                .rounded(theme.radius / 2.)
                .line_height(relative(1.))
                .text_xs()
                .whitespace_normal()
                .flex_shrink_0()
                .flex_nowrap()
                .child(kbd.label())
                .into_any_element(),
        )
    }
}
