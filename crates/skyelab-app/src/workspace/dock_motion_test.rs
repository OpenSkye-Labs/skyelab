//! Acceptance: closing a dock collapses it on Rex's spring (`ui::motion::SPRING`), asks for frames only while it
//! moves, and closes at once with Reduce Motion on.

use super::manage_harness::rig;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::dock::DockPlacement;
use std::time::Duration;

/// Frames the window asked for since the last draw (each runs the callbacks and draws again).
fn pending_frames(cx: &mut VisualTestContext) -> usize {
    let n = cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    n
}

/// The closing sidebar's drawn width, or `None` once it draws nothing.
fn closing_width(cx: &mut VisualTestContext) -> Option<f32> {
    let sel = format!("dock-closing-{}", DockPlacement::Left as u8);
    let sel: &'static str = sel.leak();
    cx.debug_bounds(sel).map(|b| f32::from(b.size.width))
}

fn close_sidebar(r: &super::manage_harness::Rig, cx: &mut VisualTestContext) {
    r.ws.update_in(cx, |ws, window, cx| {
        ws.toggle_sidebar(&super::ToggleSidebar, window, cx)
    });
    cx.run_until_parked();
}

#[gpui::test]
fn closing_sidebar_collapses_on_the_spring_then_draws_nothing(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(false));
    // Settle whatever the rig opened with.
    for _ in 0..60 {
        cx.executor().advance_clock(Duration::from_millis(16));
        if pending_frames(cx) == 0 {
            break;
        }
    }
    close_sidebar(&r, cx);
    let first = closing_width(cx).expect("the sidebar is still drawn as it starts to close");
    assert!(pending_frames(cx) > 0, "the spring asks for frames");

    cx.executor().advance_clock(Duration::from_millis(50));
    pending_frames(cx);
    // Base sizes a frame from the value the skin computed in the one before (`center_skin.rs`).
    pending_frames(cx);
    let mid = closing_width(cx).expect("still closing after 50 ms");
    assert!(
        0. < mid && mid < first,
        "part way closed after 50 ms: {mid} of {first}"
    );

    let mut settled = false;
    for _ in 0..40 {
        cx.executor().advance_clock(Duration::from_millis(16));
        if pending_frames(cx) == 0 {
            settled = true;
            break;
        }
    }
    assert!(settled, "the spring settles");
    assert_eq!(closing_width(cx), None, "a settled dock draws nothing");
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(pending_frames(cx), 0, "an idle window asks for no frames");
}

#[gpui::test]
fn reduce_motion_closes_the_dock_at_once(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    close_sidebar(&r, cx);
    pending_frames(cx);
    pending_frames(cx);
    assert_eq!(closing_width(cx), None, "no collapse with Reduce Motion");
    assert_eq!(pending_frames(cx), 0);
}

/// Base reads `closed_dock_extent` as the closed strip during a resize drag too, so an open dock must report base's
/// own value there, never its open size.
#[gpui::test]
fn an_open_dock_reports_the_base_closed_extent(cx: &mut TestAppContext) {
    use gpui_component::dock::DockAreaRenderer as _;
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(false));
    pending_frames(cx);
    r.ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.dock_skin.closed_dock_extent(DockPlacement::Left),
            ws.dock_skin.skin.closed_dock_extent(DockPlacement::Left),
            "the open sidebar reports base's closed extent"
        );
    });
}
