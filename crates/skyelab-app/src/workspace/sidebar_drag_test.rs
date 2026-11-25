//! Acceptance: dragging the sidebar's edge past `center_skin::HIDE_BELOW` hides it on release through the toggle,
//! keeping its width; above that a drag only resizes (clamped at the minimum). A hidden sidebar comes back from the
//! window's left-edge handle, by drag (at the pointer) or click (at its last width). T3's rail and restore handle.

use super::manage_harness::{Rig, rig};
use gpui::{Modifiers, MouseButton, Pixels, TestAppContext, VisualTestContext, point, px, size};
use gpui_component::dock::DockPlacement;

const Y: f32 = 300.;

fn setup(cx: &mut TestAppContext) -> (Rig, &mut VisualTestContext) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    cx.simulate_resize(size(px(1200.), px(800.)));
    cx.run_until_parked();
    (r, cx)
}

fn sidebar(r: &Rig, cx: &mut VisualTestContext) -> (bool, Pixels) {
    r.ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        (
            dock.is_dock_open(DockPlacement::Left),
            dock.dock_size(DockPlacement::Left).expect("a sidebar dock"),
        )
    })
}

fn hide_pending(r: &Rig, cx: &mut VisualTestContext) -> bool {
    r.ws.read_with(cx, |ws, _| ws.dock_skin.sidebar_drag.get().hide)
}

fn down(x: f32, cx: &mut VisualTestContext) {
    cx.simulate_mouse_down(point(px(x), px(Y)), MouseButton::Left, Modifiers::none());
}
fn drag(x: f32, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(point(px(x), px(Y)), MouseButton::Left, Modifiers::none());
}
fn up(x: f32, cx: &mut VisualTestContext) {
    cx.simulate_mouse_up(point(px(x), px(Y)), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

/// The open sidebar's handle: its last 5 px.
fn handle_x(r: &Rig, cx: &mut VisualTestContext) -> f32 {
    f32::from(sidebar(r, cx).1) - 2.5
}

#[gpui::test]
fn dragging_past_the_threshold_hides_on_release_and_keeps_the_width(cx: &mut TestAppContext) {
    let (r, cx) = setup(cx);
    assert_eq!(sidebar(&r, cx), (true, px(256.)));
    let x = handle_x(&r, cx);
    down(x, cx);
    drag(x + 1., cx);
    drag(200., cx);
    drag(40., cx);
    assert_eq!(sidebar(&r, cx), (true, px(100.)), "clamped at the minimum");
    assert!(!hide_pending(&r, cx), "above the threshold it only resizes");
    drag(10., cx);
    assert!(hide_pending(&r, cx), "past the threshold the release will hide it");
    assert_eq!(sidebar(&r, cx), (true, px(100.)), "still clamped, no jitter");
    up(10., cx);
    assert_eq!(
        sidebar(&r, cx),
        (false, px(256.)),
        "hidden, with the width it had before the drag"
    );
    assert!(!hide_pending(&r, cx));

    r.ws.update_in(cx, |ws, window, cx| {
        ws.toggle_sidebar(&super::ToggleSidebar, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(sidebar(&r, cx), (true, px(256.)), "the toggle brings it back");
}

#[gpui::test]
fn dragging_back_above_the_threshold_before_release_only_resizes(cx: &mut TestAppContext) {
    let (r, cx) = setup(cx);
    let x = handle_x(&r, cx);
    down(x, cx);
    drag(x - 1., cx);
    drag(10., cx);
    assert!(hide_pending(&r, cx));
    drag(150., cx);
    assert!(!hide_pending(&r, cx));
    up(150., cx);
    assert_eq!(sidebar(&r, cx), (true, px(150.)));
}

#[gpui::test]
fn a_press_and_one_pixel_move_changes_nothing(cx: &mut TestAppContext) {
    let (r, cx) = setup(cx);
    let x = handle_x(&r, cx);
    down(x, cx);
    drag(x + 1., cx);
    up(x + 1., cx);
    let (open, w) = sidebar(&r, cx);
    assert!(open);
    assert!(w >= px(256.) && w <= px(257.), "at most the pixel moved: {w:?}");
    // A press elsewhere far left never arms the hide.
    down(10., cx);
    drag(11., cx);
    drag(5., cx);
    assert!(!hide_pending(&r, cx));
    up(5., cx);
    assert!(sidebar(&r, cx).0);
}

fn hide(r: &Rig, cx: &mut VisualTestContext) {
    r.ws.update_in(cx, |ws, window, cx| {
        ws.toggle_sidebar(&super::ToggleSidebar, window, cx)
    });
    cx.run_until_parked();
    assert!(!sidebar(r, cx).0);
}

fn restore_center(cx: &mut VisualTestContext) -> f32 {
    let b = cx
        .debug_bounds("sidebar-restore")
        .expect("a hidden sidebar draws the restore handle");
    assert_eq!(f32::from(b.origin.x), 8.);
    assert_eq!(f32::from(b.size.width), 16.);
    f32::from(b.center().x)
}

#[gpui::test]
fn dragging_the_left_edge_brings_the_sidebar_back_at_the_pointer(cx: &mut TestAppContext) {
    let (r, cx) = setup(cx);
    assert!(cx.debug_bounds("sidebar-restore").is_none(), "no handle while open");
    hide(&r, cx);
    let x = restore_center(cx);
    let y = cx.debug_bounds("sidebar-restore").expect("handle").center().y;
    cx.simulate_mouse_down(point(px(x), y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(point(px(x + 1.), y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(point(px(30.), y), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!sidebar(&r, cx).0, "under 48 px it stays hidden");
    cx.simulate_mouse_move(point(px(300.), y), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(sidebar(&r, cx), (true, px(300.)), "shown at the pointer");
    cx.simulate_mouse_move(point(px(20.), y), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!sidebar(&r, cx).0, "back under 48 px it hides again");
    cx.simulate_mouse_move(point(px(320.), y), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(point(px(320.), y), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(sidebar(&r, cx), (true, px(320.)), "the release keeps it");
}

#[gpui::test]
fn clicking_the_left_edge_brings_the_sidebar_back_at_its_last_width(cx: &mut TestAppContext) {
    let (r, cx) = setup(cx);
    hide(&r, cx);
    let b = cx.debug_bounds("sidebar-restore").expect("handle");
    cx.simulate_click(b.center(), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(sidebar(&r, cx), (true, px(256.)));
}
