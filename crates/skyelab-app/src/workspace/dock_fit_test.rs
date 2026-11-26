//! The right panel opened on a narrow window gives up width so the thread keeps its room.

use super::layout::ToggleRightPanel;
use super::threads_test::{fixture, open_workspace, setup};
use gpui::{TestAppContext, px, size};
use gpui_component::dock::DockPlacement;

#[gpui::test]
fn a_narrow_window_keeps_room_for_the_thread(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.simulate_resize(size(px(760.), px(640.)));
    ws.update_in(cx, |ws, window, cx| {
        ws.dock.update(cx, |dock, cx| {
            dock.set_dock_size(DockPlacement::Right, px(560.), window, cx)
        })
    });
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    let (left, right) = ws.read_with(cx, |ws, cx| {
        (
            ws.dock_width(DockPlacement::Left, cx),
            ws.dock_width(DockPlacement::Right, cx),
        )
    });
    assert!(right > px(0.), "the panel opened");
    assert_eq!(left, px(0.), "the sidebar made room");
    let center = px(760.) - crate::ui::RAIL_W - left - right;
    assert!(
        center >= super::dock_fit::MIN_CENTER || right == px(280.),
        "thread keeps {center:?} beside a {right:?} panel"
    );
}

#[gpui::test]
fn a_window_too_narrow_for_the_panel_puts_it_away_on_resize(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    cx.simulate_resize(size(px(640.), px(640.)));
    // What the window-bounds observer does after a resize (it is set up by restore_layout, which tests don't call).
    ws.update_in(cx, |ws, window, cx| ws.fit_right_dock(true, window, cx));
    cx.run_until_parked();
    let open = ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(DockPlacement::Right));
    assert!(!open, "no room for the panel beside the thread: it went away");
}
