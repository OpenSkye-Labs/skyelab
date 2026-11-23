//! Tools open in the right dock, beside the thread, and the dock toggles follow the docks.

use super::layout::{DockToggles, ToggleRightPanel};
use super::threads_test::{fixture, open_workspace, project_thread, setup};
use super::*;
use gpui::TestAppContext;
use gpui_component::dock::DockPlacement;

fn right_open(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> (bool, bool) {
    ws.read_with(cx, |ws, cx| {
        (
            ws.dock.read(cx).is_dock_open(DockPlacement::Right),
            cx.global::<DockToggles>().right,
        )
    })
}

#[gpui::test]
fn changes_open_in_the_right_dock_and_the_toggle_follows(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    assert_eq!(right_open(&ws, cx), (false, false), "starts closed");
    let thread = project_thread(&f, &ws, cx);
    ws.update_in(cx, |ws, window, cx| ws.open_changes(&thread, window, cx));
    cx.run_until_parked();
    assert_eq!(
        right_open(&ws, cx),
        (true, true),
        "Changes opened the right dock"
    );
    ws.read_with(cx, |ws, cx| {
        assert!(!ws.dock.read(cx).is_empty(DockPlacement::Right, cx));
        assert!(ws.right_empty.is_none(), "the placeholder left");
    });
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    assert_eq!(right_open(&ws, cx), (false, false), "toggled closed");
    // Opening Changes again shows the dock with it.
    ws.update_in(cx, |ws, window, cx| ws.open_changes(&thread, window, cx));
    cx.run_until_parked();
    assert_eq!(right_open(&ws, cx), (true, true));
}
