//! gpui-component's dock skin, except that the center panel and side chat panels are drawn uncached. The skin draws
//! every panel as a cached view, and a cached view that misses its cache builds its whole subtree again (GPUI sets
//! `window.refreshing`). The composer's text area is a view, so each caret blink dirtied its panel and rebuilt every
//! timeline row. Uncached, these panels cache what they need themselves: a thread page's timeline is a cached view
//! (`timeline_view.rs`), and the center's other pages are drawn cached by `Center` (docs/gpui/redraw.md).
//!
//! A dock that closes (the sidebar, the tools dock, the terminal) collapses on Rex's spring (`ui::motion::SPRING`)
//! instead of vanishing: base asks [`DockAreaRenderer::closed_dock_extent`] how much of a closed dock to draw, and
//! this skin answers with the spring's last value until it settles. Opening stays instant, as Rex's lands most of
//! the way on its first frame.

use gpui::{
    AnyElement, AnyView, App, Axis, Div, EntityId, Pixels, Stateful, Window, div, prelude::*,
};
use gpui_base::ResizeHandleContext;
use gpui_base::dock::PanelView;
use gpui_component::dock::{
    DockAreaRenderer, DockContext, DockPlacement, DockSkin, DropIndicator, NodeId, PanelState,
    TabGroupContext, TabGroupRenderer,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// [`DockSkin`] with the panel `center` and every side chat panel drawn uncached.
pub(super) struct CenterSkin {
    pub skin: Rc<DockSkin>,
    pub center: EntityId,
    /// Left, right, bottom: the extent each dock was last drawn at, and its size when last open.
    pub drawn: [Cell<Drawn>; 3],
}

#[derive(Clone, Copy, Default)]
pub(super) struct Drawn {
    /// `None` once a closed dock has settled at its closed extent.
    extent: Option<Pixels>,
    open: Pixels,
}

/// The spring's key: one per dock.
fn extent_id(at: DockPlacement) -> gpui::ElementId {
    gpui::ElementId::NamedInteger("dock-extent".into(), at as u64)
}

impl CenterSkin {
    pub fn new(skin: Rc<DockSkin>, center: EntityId) -> Self {
        Self {
            skin,
            center,
            drawn: Default::default(),
        }
    }

    fn drawn(&self, at: DockPlacement) -> Option<&Cell<Drawn>> {
        match at {
            DockPlacement::Left => Some(&self.drawn[0]),
            DockPlacement::Right => Some(&self.drawn[1]),
            DockPlacement::Bottom => Some(&self.drawn[2]),
            DockPlacement::Center => None,
        }
    }

    /// A closing dock: its content held at the size it had open, pinned to the edge beside the center and clipped
    /// by base's frame, so it slides out without laying out (or resizing a terminal) on every frame.
    fn render_closing(
        &self,
        at: DockPlacement,
        cell: &Cell<Drawn>,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut drawn = cell.get();
        let closed = self.skin.closed_dock_extent(at);
        let now = gpui_base::spring(
            extent_id(at),
            closed,
            crate::ui::motion::SPRING_PX,
            window,
            cx,
        );
        drawn.extent = (now > closed).then_some(now);
        if drawn.extent.is_none() {
            // Base sized this frame from the last value; one more frame draws the closed extent.
            window.request_animation_frame();
        }
        cell.set(drawn);
        let open = drawn.open;
        div()
            .debug_selector(move || format!("dock-closing-{}", at as u8))
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .map(|d| match at {
                        DockPlacement::Left => d.top_0().bottom_0().right_0().w(open),
                        DockPlacement::Right => d.top_0().bottom_0().left_0().w(open),
                        _ => d.left_0().right_0().top_0().h(open),
                    })
                    .child(content),
            )
            .into_any_element()
    }
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
        let closed = self.skin.closed_dock_extent(placement);
        self.drawn(placement)
            .and_then(|d| d.get().extent)
            .map_or(closed, |e| e.max(closed))
    }
    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let at = dock.placement();
        let Some(cell) = self.drawn(at) else {
            return self.skin.render_dock(dock, content, window, cx);
        };
        if dock.is_open() {
            let size = dock.size();
            // The spring sits on the open size (no travel while open, so a resize drag never lags), and a close
            // starts from it.
            gpui_base::spring(
                extent_id(at),
                size,
                crate::ui::motion::SPRING_PX.with_travel(false),
                window,
                cx,
            );
            cell.set(Drawn {
                // With Reduce Motion a close draws nothing from its first frame.
                extent: (!cx.reduce_motion()).then_some(size),
                open: size,
            });
            return self.skin.render_dock(dock, content, window, cx);
        }
        if cell.get().extent.is_some() {
            return self.render_closing(at, cell, content, window, cx);
        }
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
        self.tabs.render_tab_bar(group, window, cx)
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
