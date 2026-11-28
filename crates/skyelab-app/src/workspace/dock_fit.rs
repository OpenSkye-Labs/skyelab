//! The right panel never takes the thread's room: on a narrow window (or one restored narrower than the panel it
//! saved) the panel gives up width until the page keeps [`MIN_CENTER`], down to [`MIN_RIGHT`]. A window too narrow
//! even for that puts the panel away when it opens or shrinks; a panel the user just opened hides the sidebar instead,
//! then keeps at least `MIN_RIGHT`.

use super::Workspace;
use crate::ui::dock::DockPlacement;
use gpui::{Context, Pixels, Window, px};

/// The thread page's narrowest: the composer's chips and the header still fit.
pub(super) const MIN_CENTER: Pixels = px(360.);
/// The right panel's narrowest after fitting; below that the page gets less than `MIN_CENTER`.
const MIN_RIGHT: Pixels = px(280.);

impl Workspace {
    /// `close`: put the panel away when it can't keep `MIN_RIGHT` (a restore or a resize, not the user's toggle).
    pub(super) fn fit_right_dock(
        &mut self,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let right = self.dock_width(DockPlacement::Right, cx);
        let room = window.viewport_size().width
            - crate::ui::RAIL_W
            - self.dock_width(DockPlacement::Left, cx)
            - MIN_CENTER;
        if right == Pixels::ZERO || right <= room {
            return;
        }
        if close && room < MIN_RIGHT {
            // Its width stays as it was, for when the window is wide again.
            return self.toggle(DockPlacement::Right, window, cx);
        }
        if room < MIN_RIGHT && self.dock_width(DockPlacement::Left, cx) > Pixels::ZERO {
            // The user opened the panel: the sidebar makes room (the toggle fits the panel again).
            return self.toggle(DockPlacement::Left, window, cx);
        }
        let w = room.max(MIN_RIGHT);
        self.dock.update(cx, |dock, cx| {
            dock.set_dock_size(DockPlacement::Right, w, window, cx)
        });
    }
}
