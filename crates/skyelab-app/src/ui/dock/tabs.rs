//! Our look for one tab group: its title bar (one panel) or tab bar (several), the toolbar with the zoom button and
//! the "…" menu, and the shown panel drawn as a cached view.

use super::{
    ClosePanel, PanelControl, PanelHandle, TabGroupContext, TabGroupRenderer, ToggleZoom,
    panel_title, zoom_control,
};
use crate::ui::{
    A11yScrollArea as _, ActiveTheme as _, Button, Icon, PopupMenuItem, SkyeIcon, h_flex,
};
use gpui::{
    Anchor, AnyElement, AnyView, App, Div, Empty, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, ScrollHandle, Stateful, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Window, div, prelude::FluentBuilder as _, px, relative, rems,
};
use gpui_base::{InteractiveElementExt as _, ScrollableMask};
use std::{cell::Cell, rc::Rc};

/// A tab's height, and its label box's (one pixel under the bar's bottom border).
const TAB_H: Pixels = px(32.);
const TAB_INNER_H: Pixels = px(30.);
const TAB_PAD_X: Pixels = px(12.);

/// The toolbar's ghost button at gpui-component's extra-small size (1.25 rem, a 0.75 rem icon, the theme's radius),
/// so the tab bar keeps its look: smaller than our smallest `Size`, and not a tab stop.
fn tool_button(id: &'static str, icon: SkyeIcon, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .tab_stop(false)
        .size(rems(1.25))
        .px_0()
        .rounded(cx.theme().radius)
        // Held (zoomed, menu open) in gpui-component's ghost fill, as these buttons always were.
        .held_bg(cx.theme().secondary_active)
        .child(
            Icon::new(icon)
                .size_3()
                .text_color(cx.theme().secondary_foreground),
        )
}

/// One group's renderer; base builds one per group, so the tab bar's scroll position is that group's.
#[derive(Default)]
pub(super) struct Tabs {
    scroll: ScrollHandle,
    /// The shown tab the last frame drew: a change scrolls the new one into view.
    last_active: Rc<Cell<Option<usize>>>,
}

impl Tabs {
    /// The zoom button (when the panel offers it there) and the "…" menu.
    fn toolbar(&self, group: &TabGroupContext, cx: &App) -> Div {
        if group.is_collapsed() {
            return div();
        }
        let zoomed = group.is_zoomed();
        let control = zoom_control(group, cx);
        let toolbar_zoom = control == Some(PanelControl::Both);
        let menu_zoom = control.is_some();
        let closable = group.is_closable();
        let panel = group.active_panel().and_then(PanelHandle::of).cloned();
        let focus = group.active_panel().map(|p| p.focus_handle(cx));
        let zoom_label = if zoomed { "Zoom Out" } else { "Zoom In" };
        let zoom = match (zoomed, toolbar_zoom) {
            (true, _) => Some(("zoom-out", SkyeIcon::Collapse)),
            (false, true) => Some(("zoom-in", SkyeIcon::Expand)),
            (false, false) => None,
        };
        h_flex()
            .gap_1()
            .occlude()
            .when_some(zoom, |d, (id, icon)| {
                let group = group.clone();
                d.child(
                    tool_button(id, icon, cx)
                        .tooltip_with_action(zoom_label, &ToggleZoom, None)
                        .selected(zoomed)
                        .debug_selector(|| "dock-tab-bar-zoom-control".into())
                        .on_click(move |_, window, cx| group.toggle_zoom(window, cx)),
                )
            })
            .child(
                tool_button("menu", SkyeIcon::Ellipsis, cx)
                    .accessibility_label("More actions")
                    .debug_selector(|| "dock-tab-bar-menu".into())
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                        let menu = match &panel {
                            Some(p) => p.0.dropdown_menu(menu, window, cx),
                            None => menu,
                        };
                        // Sent from the shown panel, so a panel's own handler (an unsaved preview) runs first.
                        let send = |action: &'static dyn gpui::Action| {
                            let focus = focus.clone();
                            move |_: &gpui::ClickEvent, window: &mut Window, cx: &mut App| {
                                if let Some(f) = &focus {
                                    f.dispatch_action(action, window, cx);
                                }
                            }
                        };
                        let menu = menu.separator().item(
                            PopupMenuItem::new(zoom_label)
                                .disabled(!menu_zoom)
                                .on_click(send(&ToggleZoom)),
                        );
                        if closable {
                            menu.separator()
                                .item(PopupMenuItem::new("Close").on_click(send(&ClosePanel)))
                        } else {
                            menu
                        }
                    }),
            )
    }

    /// A group of one panel: its title, the panel's suffix and the toolbar.
    fn title_bar(
        &self,
        group: &TabGroupContext,
        ix: usize,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let panel = &group.panels()[ix];
        let suffix = PanelHandle::of(panel).and_then(|h| h.0.title_suffix(window, cx));
        h_flex()
            .justify_between()
            .h(px(30.))
            .py_2()
            .pl(rems(0.75))
            .pr(rems(0.5))
            .child(
                div()
                    .id("tab")
                    .flex_1()
                    .min_w_16()
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(panel_title(panel, window, cx)),
            )
            .children(suffix)
            .child(
                h_flex()
                    .flex_shrink_0()
                    .ml(rems(0.25))
                    .gap_1()
                    .child(self.toolbar(group, cx)),
            )
            .into_any_element()
    }

    /// A group of several panels: a scrolling row of tabs, then the panel's suffix and the toolbar.
    fn tab_bar(
        &self,
        group: &TabGroupContext,
        visible: &[usize],
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let collapsed = group.is_collapsed();
        let active_ix = group.active_ix();
        let shown = group.active_panel().map(|p| p.panel_id(cx));
        let shown_ix =
            shown.and_then(|id| group.panels().iter().position(|p| p.panel_id(cx) == id));
        if self.last_active.replace(Some(active_ix)) != Some(active_ix)
            && let Some(at) = visible.iter().position(|ix| *ix == active_ix)
        {
            self.scroll.scroll_to_item(at);
        }
        let tabs: Vec<AnyElement> = visible
            .iter()
            .map(|&ix| {
                let selected = !collapsed && Some(ix) == shown_ix;
                self.tab(
                    group,
                    ix,
                    selected,
                    Some(&ix) == visible.first(),
                    window,
                    cx,
                )
            })
            .collect();
        let suffix = (!collapsed).then(|| {
            let panel_suffix = group
                .active_panel()
                .and_then(PanelHandle::of)
                .and_then(|h| h.0.title_suffix(window, cx));
            let toolbar = self.toolbar(group, cx);
            let theme = cx.theme();
            h_flex()
                .items_center()
                .top_0()
                .border_l_1()
                .border_b_1()
                .h_full()
                .border_color(theme.border)
                .bg(theme.tab_bar)
                .px_2()
                .gap_1()
                .children(panel_suffix)
                .child(toolbar)
        });
        let theme = cx.theme();
        let empty = div()
            .id("tab-bar-empty-space")
            .h_full()
            .flex_grow(1.)
            .min_w_16();
        gpui_base::Tabs::new("tab-bar")
            .group("tab-bar")
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .bg(theme.tab_bar)
            .text_color(theme.tab_foreground)
            .child(
                div()
                    .id("border-b")
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .size_full()
                    .border_b_1()
                    .border_color(theme.border),
            )
            .child(
                h_flex()
                    .id("tabs")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_hidden()
                    .child(
                        h_flex()
                            .id("tabs-inner")
                            .flex_1()
                            .relative()
                            .overflow_x_scroll()
                            .lock_scroll_axis()
                            .track_scroll(&self.scroll)
                            .children(tabs)
                            .when(suffix.is_some(), |d| {
                                d.child(div().flex_shrink_0().child(empty))
                            }),
                    )
                    .child(ScrollableMask::vertical_wheel(&self.scroll)),
            )
            .children(suffix)
            .into_any_element()
    }

    fn tab(
        &self,
        group: &TabGroupContext,
        ix: usize,
        selected: bool,
        first: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let panel = &group.panels()[ix];
        let collapsed = group.is_collapsed();
        let label = PanelHandle::of(panel)
            .and_then(|h| h.0.tab_name(cx))
            .unwrap_or_else(|| panel.panel_name(cx).into());
        let title = panel_title(panel, window, cx);
        let theme = cx.theme();
        // The first shown tab meets the bar's start edge, so it drops that border.
        let start_border = if first { px(0.) } else { px(1.) };
        let (ring, border, hover_fg) = (theme.ring, theme.border, theme.tab_active_foreground);
        let group_ = group.clone();
        let focus = panel.focus_handle(cx);
        let last_active = self.last_active.clone();
        gpui_base::Tab::new(ix)
            .selected(selected)
            .accessibility_label(label)
            .focus_visible(move |s| {
                s.shadow(vec![gpui::BoxShadow {
                    color: ring,
                    offset: gpui::point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(2.),
                    inset: true,
                }])
            })
            .relative()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_1()
            .items_center()
            .flex_shrink_0()
            .h(TAB_H)
            .overflow_hidden()
            .text_sm()
            .border_l(start_border)
            .border_r(px(1.))
            .map(|t| {
                if selected {
                    t.text_color(theme.tab_active_foreground)
                        .bg(theme.tab_active)
                        .border_color(border)
                } else {
                    t.text_color(theme.tab_foreground)
                        .border_color(gpui::transparent_black())
                        .hover(move |s| s.text_color(hover_fg))
                }
            })
            .debug_selector(move || format!("dock-tab-{ix}"))
            .child(
                h_flex()
                    .flex_1()
                    .h(TAB_INNER_H)
                    .line_height(relative(1.25))
                    .whitespace_nowrap()
                    .justify_center()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .px(TAB_PAD_X)
                    .child(title),
            )
            .on_click(move |_, window, cx| {
                // The tab bar shows a clicked tab itself; the change must not scroll it again.
                last_active.set(Some(ix));
                group_.select_tab(ix, window, cx);
                // Base moves focus into the panel only when the tab changes; a click on the shown tab focuses it too.
                if !collapsed {
                    focus.focus(window, cx);
                }
            })
            .into_any_element()
    }
}

impl TabGroupRenderer for Tabs {
    fn frame(&self, group: &TabGroupContext, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let control = zoom_control(group, cx);
        div()
            .id("tab-panel")
            .bg(cx.theme().background)
            // A collapsed group is a strip with no content to act on.
            .when(!group.is_collapsed(), |d| {
                let (zoom, close) = (group.clone(), group.clone());
                d.on_action(move |_: &ToggleZoom, window, cx| {
                    // A panel that offers no control isn't zoomed in by the action either; zooming out always works.
                    if zoom.is_zoomed() || control.is_some() {
                        zoom.toggle_zoom(window, cx);
                    }
                })
                .on_action(move |_: &ClosePanel, window, cx| {
                    if let Some(panel) = close.active_panel() {
                        close.close(panel.panel_id(cx), window, cx);
                    }
                })
            })
    }

    fn content_frame(&self, group: &TabGroupContext, _: &mut Window, _: &mut App) -> Stateful<Div> {
        div()
            .id("active-panel")
            .when(group.panels().len() > 1, |d| d.pt_2())
    }

    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let visible: Vec<usize> = group
            .panels()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.visible(cx))
            .map(|(ix, _)| ix)
            .collect();
        match visible.as_slice() {
            [] => Empty.into_any_element(),
            [ix] => {
                let panel = &group.panels()[*ix];
                if PanelHandle::of(panel).is_some_and(|h| !h.0.title_bar(cx)) {
                    return Empty.into_any_element();
                }
                self.title_bar(group, *ix, window, cx)
            }
            _ => self.tab_bar(group, &visible, window, cx),
        }
    }

    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        if group.is_collapsed() {
            return Empty.into_any_element();
        }
        div()
            .id("tab-content")
            .overflow_y_scroll()
            .a11y_scroll_area()
            .overflow_x_hidden()
            .flex_1()
            .child(crate::ui::cached_view(
                panel,
                StyleRefinement::default().absolute().size_full(),
                window,
            ))
            .into_any_element()
    }
}
