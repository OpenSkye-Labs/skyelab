//! Tools open in the right dock, beside the thread, and the dock toggles follow the docks.

use super::layout::ToggleRightPanel;
use super::threads_test::{fixture, open_workspace, project_thread, setup};
use super::*;
use gpui::TestAppContext;
use gpui_component::dock::DockPlacement;

fn right_open(ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext) -> (bool, bool) {
    ws.read_with(cx, |ws, cx| {
        (
            ws.dock.read(cx).is_dock_open(DockPlacement::Right),
            super::layout::docks_of(ws.window_handle, cx).right,
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
        assert!(
            ws.changes_views.len() == 1,
            "Changes joined the surfaces tab"
        );
    });
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    assert_eq!(right_open(&ws, cx), (false, false), "toggled closed");
    // Opening Changes again shows the dock with it.
    ws.update_in(cx, |ws, window, cx| ws.open_changes(&thread, window, cx));
    cx.run_until_parked();
    assert_eq!(right_open(&ws, cx), (true, true));
    // Closing the last tool leaves the "Open a surface" tab, which always stays.
    ws.update_in(cx, |ws, window, cx| {
        let changes = ws.changes_views[0].upgrade().expect("changes tab");
        ws.dock
            .update(cx, |dock, cx| dock.remove_panel(changes, window, cx));
    });
    cx.run_until_parked();
    assert_eq!(
        right_open(&ws, cx),
        (true, true),
        "Open a surface still shows"
    );
    ws.read_with(cx, |ws, cx| {
        assert!(!ws.dock.read(cx).is_empty(DockPlacement::Right, cx))
    });
}
