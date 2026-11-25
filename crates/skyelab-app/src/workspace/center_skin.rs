//! gpui-component's dock skin, except that the center panel and side chat panels are drawn uncached. The skin draws
//! every panel as a cached view, and a cached view that misses its cache builds its whole subtree again (GPUI sets
//! `window.refreshing`). The composer's text area is a view, so each caret blink dirtied its panel and rebuilt every
//! timeline row. Uncached, these panels cache what they need themselves: a thread page's timeline is a cached view
//! (`timeline_view.rs`), and the center's other pages are drawn cached by `Center` (docs/gpui/redraw.md).

use gpui::{
    AnyElement, AnyView, App, Axis, Div, EntityId, MouseButton, Pixels, Stateful, Window, div,
    prelude::*,
};
use gpui_base::ResizeHandleContext;
use gpui_base::dock::PanelView;
use gpui_component::dock::{
    DockAreaRenderer, DockContext, DockPlacement, DockSkin, DropIndicator, NodeId, PanelState,
    TabGroupContext, TabGroupRenderer,
};
use std::rc::Rc;
use std::sync::Arc;

/// [`DockSkin`] with the panel `center` and every side chat panel drawn uncached.
pub(super) struct CenterSkin {
    pub skin: Rc<DockSkin>,
    pub center: EntityId,
}

impl DockAreaRenderer for CenterSkin {
    fn frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.skin.frame(window, cx)
    }
    fn split_frame(
        &self,
        node: NodeId,
        axis: Axis,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.skin.split_frame(node, axis, window, cx)
    }
    fn center_frame(&self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.skin.center_frame(window, cx)
    }
    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.skin.render_split_handle(handle, window, cx)
    }
    fn closed_dock_extent(&self, placement: DockPlacement) -> Pixels {
        self.skin.closed_dock_extent(placement)
    }
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self.skin.render_dock(dock, content, window, cx)
    }
    fn build_placeholder(
        &self,
        state: &PanelState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<dyn PanelView>> {
        self.skin.build_placeholder(state, window, cx)
    }
    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(CenterTabs {
            tabs: self.skin.tab_group_renderer(),
            center: self.center,
        })
    }
}

struct CenterTabs {
    tabs: Rc<dyn TabGroupRenderer>,
    center: EntityId,
}

impl TabGroupRenderer for CenterTabs {
    fn frame(&self, group: &TabGroupContext, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        self.tabs.frame(group, window, cx)
    }
    fn content_frame(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.tabs.content_frame(group, window, cx)
    }
    fn render_tab_bar(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let bar = self.tabs.render_tab_bar(group, window, cx);
        let Some(active) = group
            .active_panel()
            .cloned()
            .filter(|_| !group.is_collapsed())
        else {
            return bar;
        };
        // A press on a tab focuses the tab, and gpui-base's `select_tab` moves focus into the panel only when the tab
        // changes: a click on the tab already shown left the keys (mod+F, Esc) outside the panel. The bar's toolbar
        // occludes, so its menu keeps the focus it takes.
        let panels = group.panels().to_vec();
        div()
            .w_full()
            .flex_shrink_0()
            .child(bar)
            .on_mouse_up(MouseButton::Left, move |_, window, cx| {
                let held = panels.iter().any(|p| {
                    let h = p.focus_handle(cx);
                    h.is_focused(window) || h.contains_focused(window, cx)
                });
                if !held {
                    active.focus_handle(cx).focus(window, cx);
                }
            })
            .into_any_element()
    }
    fn render_active_panel(
        &self,
        panel: AnyView,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        // A side chat is a thread page too: its composer blinks under the right dock's panel.
        let thread_page = panel.entity_id() == self.center
            || panel
                .clone()
                .downcast::<super::side_chat::SideChatPanel>()
                .is_ok();
        if !thread_page || group.is_collapsed() {
            return self.tabs.render_active_panel(panel, group, window, cx);
        }
        // The skin's box (`render_active_panel` in gpui-component's tab_panel.rs) around the view, not cached.
        div()
            .id("tab-content")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .flex_1()
            .child(div().absolute().size_full().child(panel))
            .into_any_element()
    }
    fn render_drop_indicator(
        &self,
        indicator: DropIndicator,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.tabs.render_drop_indicator(indicator, window, cx)
    }
    fn render_empty(
        &self,
        group: &TabGroupContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        self.tabs.render_empty(group, window, cx)
    }
}
