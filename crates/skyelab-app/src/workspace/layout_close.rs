//! Closing from the right panel: cmd-W closes its active tab, Esc on a tab without its own Esc hides it.

use super::Workspace;
use super::layout::{CloseTab, HideRightPanel};
use crate::ui::dock::DockPlacement;
use gpui::{Context, Window};

impl Workspace {
    /// Closes the right panel's active tab through the dock's `ClosePanel`, sent from that tab so a tab that asks
    /// first (an unsaved Preview) still asks. Nothing happens while the right panel is closed, as in T3.
    pub(super) fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tab, _)) = self.dock_focus(DockPlacement::Right, window, cx) else {
            return;
        };
        let dock = self.dock.clone();
        // Deferred: closing a tab emits dock events this workspace handles.
        window.defer(cx, move |window, cx| {
            // The dock is locked (its "…" menus offer no Close), and a locked tab group refuses every close.
            dock.update(cx, |d, cx| d.set_locked(false, window, cx));
            tab.dispatch_action(&crate::ui::dock::ClosePanel, window, cx);
            dock.update(cx, |d, cx| d.set_locked(true, window, cx));
        });
    }

    /// Esc on a right-panel tab that has no Esc of its own (Open a surface): hide the panel, keys back to the window.
    pub(super) fn hide_right(
        &mut self,
        _: &HideRightPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.unzoom(window, cx) {
            return;
        }
        if self.dock.read(cx).is_dock_open(DockPlacement::Right) {
            self.toggle(DockPlacement::Right, window, cx);
            self.focus.focus(window, cx);
        }
    }
}
