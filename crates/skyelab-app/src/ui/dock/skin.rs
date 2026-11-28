//! Our look for gpui-base's dock area: the frames, the split dividers, each dock's resize handle and the closed bottom
//! dock's strip. Tab groups are drawn by `tabs.rs`.

use super::{
    CLOSED_BOTTOM_STRIP, DockArea, DockAreaRenderer, DockContext, DockEvent, DockPlacement, NodeId,
    TabGroupRenderer,
};
use crate::ui::ActiveTheme as _;
use gpui::{
    AnyElement, App, AppContext as _, Axis, Context, Div, Empty, InteractiveElement as _,
    IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Render, Stateful,
    Styled as _, WeakEntity, Window, canvas, div, px,
};
use gpui_base::{HandleEdge, LayoutDirection, ResizeHandleContext};
use std::{cell::Cell, rc::Rc};

/// Our dock look. Install it while the area is built: it needs the area's own handle.
pub struct Skin {
    area: WeakEntity<DockArea>,
    /// The dock whose handle is being dragged; only one can be.
    resizing: Rc<Cell<Option<DockPlacement>>>,
}

impl Skin {
    pub fn new(cx: &mut Context<DockArea>) -> Rc<Self> {
        Rc::new(Self {
            area: cx.weak_entity(),
            resizing: Rc::default(),
        })
    }
}

/// What a dock's resize handle drags; the handle itself is the affordance.
#[derive(Clone)]
struct ResizeDock;

impl Render for ResizeDock {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl DockAreaRenderer for Skin {
    fn closed_dock_extent(&self, placement: DockPlacement) -> Pixels {
        match placement {
            DockPlacement::Bottom => CLOSED_BOTTOM_STRIP,
            _ => px(0.),
        }
    }

    fn frame(&self, _: &mut Window, _: &mut App) -> Stateful<Div> {
        div().id("dock-area")
    }

    fn center_frame(&self, _: &mut Window, _: &mut App) -> Stateful<Div> {
        div().id("dock-area-center")
    }

    fn render_split_handle(
        &self,
        handle: &ResizeHandleContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        Some(super::handle::render(handle, window, cx))
    }

    fn split_frame(&self, node: NodeId, _: Axis, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        div()
            .id(("dock-split-frame", node.as_u64()))
            .bg(cx.theme().tab_bar)
    }

    fn render_dock(
        &self,
        dock: &DockContext,
        content: AnyElement,
        _: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        div()
            .flex()
            .size_full()
            .relative()
            .child(content)
            .child(self.resize_handle(dock, cx))
            .child(self.resize_tracker(dock))
            .into_any_element()
    }

    fn tab_group_renderer(&self) -> Rc<dyn TabGroupRenderer> {
        Rc::new(super::tabs::Tabs::default())
    }
}

impl Skin {
    fn resize_handle(&self, dock: &DockContext, cx: &App) -> impl IntoElement + use<> {
        let placement = dock.placement();
        // One id per dock: they render under one stateful parent, and a shared id would share their drag state.
        let id = match placement {
            DockPlacement::Left => "resize-handle-left",
            DockPlacement::Right => "resize-handle-right",
            DockPlacement::Bottom => "resize-handle-bottom",
            DockPlacement::Center => "resize-handle-center",
        };
        // The dock clips to its box, so each handle hugs the dock's edge toward the center.
        let edge = match placement.physical(LayoutDirection::global(cx)) {
            DockPlacement::Left => HandleEdge::Trailing,
            _ => HandleEdge::Leading,
        };
        let resizing = self.resizing.clone();
        gpui_base::resize_handle(id, placement.axis())
            .with_appearance(Rc::new(|h, window, cx| {
                Some(super::handle::render(h, window, cx))
            }))
            .inside(edge)
            .on_drag(ResizeDock, move |drag, _, _, cx| {
                cx.stop_propagation();
                resizing.set(Some(placement));
                cx.new(|_| drag.clone())
            })
    }
}

impl Skin {
    /// Turns the window's pointer moves into a dock resize while a handle is dragged: the pointer leaves the handle,
    /// so only window-level listeners, registered at paint, can follow it.
    fn resize_tracker(&self, dock: &DockContext) -> impl IntoElement + use<> {
        let (dock, resizing, area) = (dock.clone(), self.resizing.clone(), self.area.clone());
        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                let placement = dock.placement();
                let (moved, moving) = (dock.clone(), resizing.clone());
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase.bubble() && moving.get() == Some(placement) {
                        // Base opens, closes and sizes the dock from the pointer.
                        moved.resize_to(e.position, window, cx);
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
                    if !phase.bubble() || resizing.get() != Some(placement) {
                        return;
                    }
                    resizing.set(None);
                    dock.end_resize(window, cx);
                    // A dock's size isn't in the layout tree, so nothing else tells a subscriber to save it.
                    let _ = area.update(cx, |_, cx| cx.emit(DockEvent::LayoutChanged));
                });
            },
        )
    }
}
