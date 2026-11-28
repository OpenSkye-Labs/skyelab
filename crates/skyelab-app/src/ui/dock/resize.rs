//! Dock sizes: what a dock remembers, how a drag of its edge resizes it, and when the bottom dock snaps closed.

use super::{DockArea, DockEvent, DockPlacement};
use gpui::{App, Context, Pixels, Point, WeakEntity, Window, px};

/// The narrowest a dock or the center gets.
pub const PANEL_MIN_SIZE: Pixels = px(100.);

/// What a closed bottom dock keeps, so its bar stays clickable.
pub const CLOSED_BOTTOM_STRIP: Pixels = px(29.);

#[derive(Clone, Copy)]
pub(super) struct Dock {
    pub open: bool,
    /// Its size when open, never under [`PANEL_MIN_SIZE`].
    size: Pixels,
    /// What a drag shows under the minimum; the release settles it.
    live: Option<Pixels>,
}

impl Dock {
    pub fn new(size: Option<Pixels>) -> Self {
        Self {
            open: true,
            size: size.unwrap_or(PANEL_MIN_SIZE * 2.).max(PANEL_MIN_SIZE),
            live: None,
        }
    }
}

#[derive(Clone)]
pub struct DockContext {
    area: WeakEntity<DockArea>,
    placement: DockPlacement,
    size: Pixels,
    open: bool,
}

impl DockContext {
    pub fn placement(&self) -> DockPlacement {
        self.placement
    }

    /// Its width (left, right) or height (bottom) when open.
    pub fn size(&self) -> Pixels {
        self.size
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The dragged edge is at `pointer`, in window coordinates.
    pub fn resize_to(&self, pointer: Point<Pixels>, window: &mut Window, cx: &mut App) {
        _ = self.area.update(cx, |area, cx| {
            area.resize_dock(self.placement, pointer, window, cx)
        });
    }

    pub fn end_resize(&self, window: &mut Window, cx: &mut App) {
        _ = self
            .area
            .update(cx, |area, cx| area.end_resize(self.placement, window, cx));
    }
}

impl DockArea {
    pub fn dock_size(&self, at: DockPlacement) -> Option<Pixels> {
        self.docks[at as usize].map(|d| d.size)
    }

    pub fn set_dock_size(
        &mut self,
        at: DockPlacement,
        size: Pixels,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = size.max(PANEL_MIN_SIZE);
        if let Some(dock) = &mut self.docks[at as usize]
            && dock.size != size
        {
            dock.size = size;
            cx.emit(DockEvent::LayoutChanged);
            cx.notify();
        }
    }

    pub(super) fn dock_context(
        &self,
        at: DockPlacement,
        cx: &Context<Self>,
    ) -> Option<DockContext> {
        self.docks[at as usize].map(|d| DockContext {
            area: cx.weak_entity(),
            placement: at,
            size: d.live.unwrap_or(d.size),
            open: d.open,
        })
    }

    /// A side dock leaves the center and the dock opposite (open or not) their room. The bottom dock follows the
    /// pointer under its minimum down to its strip, so closing it by drag is one motion.
    fn resize_dock(
        &mut self,
        at: DockPlacement,
        pointer: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let area = self.bounds;
        let other = |at| self.dock_size(at).unwrap_or_default();
        let (size, room) = match at {
            DockPlacement::Left => (
                pointer.x - area.left(),
                area.size.width - other(DockPlacement::Right),
            ),
            DockPlacement::Right => (
                area.right() - pointer.x,
                area.size.width - other(DockPlacement::Left),
            ),
            DockPlacement::Bottom => (area.bottom() - pointer.y, area.size.height),
            DockPlacement::Center => return,
        };
        let size = size.min((room - PANEL_MIN_SIZE).max(PANEL_MIN_SIZE));
        let strip = self.renderer.closed_dock_extent(at);
        let Some(dock) = &mut self.docks[at as usize] else {
            return;
        };
        let was_open = dock.open;
        if at == DockPlacement::Bottom && size < PANEL_MIN_SIZE {
            dock.open = size > strip;
            dock.live = Some(size.max(strip));
        } else {
            dock.open = true;
            dock.live = None;
            dock.size = size.max(PANEL_MIN_SIZE);
        }
        if dock.open != was_open {
            self.schedule_active_sync(window, cx);
        }
        cx.notify();
    }

    fn end_resize(&mut self, at: DockPlacement, window: &mut Window, cx: &mut Context<Self>) {
        let strip = self.renderer.closed_dock_extent(at);
        if let Some(dock) = &mut self.docks[at as usize]
            && let Some(live) = dock.live.take()
        {
            dock.open = live >= (strip + PANEL_MIN_SIZE) / 2.;
            if dock.open {
                dock.size = PANEL_MIN_SIZE;
            }
            self.schedule_active_sync(window, cx);
        }
        cx.emit(DockEvent::LayoutChanged);
        cx.notify();
    }
}
