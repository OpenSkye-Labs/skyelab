//! Acceptance (QA #3): the dock "…" menu's Zoom item works. gpui-component always adds it, disabled unless the panel
//! can zoom, and a disabled item looks enabled and keeps the menu open. So every dock panel zooms: the item (and
//! the ToggleZoom action it sends to the focused panel) fills the window with the panel's group and back.

use super::devices_tests::{Fake, open};
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::*;
use gpui_component::dock::ToggleZoom;
use std::sync::Arc;

fn zoomed(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> bool {
    ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_zoomed())
}

#[gpui::test]
fn the_terminal_zooms_in_and_out(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    cx.run_until_parked();
    cx.dispatch_action(ToggleZoom);
    cx.run_until_parked();
    assert!(zoomed(&ws, cx), "Zoom In fills the window with the terminal");
    cx.dispatch_action(ToggleZoom);
    cx.run_until_parked();
    assert!(!zoomed(&ws, cx), "Zoom Out puts it back");
}

#[gpui::test]
fn the_devices_tab_zooms(cx: &mut gpui::TestAppContext) {
    let (ws, panel, cx) = open(cx, Arc::<Fake>::default());
    panel.update_in(cx, |p, window, cx| p.focus_handle(cx).focus(window, cx));
    cx.run_until_parked();
    cx.dispatch_action(ToggleZoom);
    cx.run_until_parked();
    assert!(zoomed(&ws, cx), "Zoom In works on the Devices tab");
}
