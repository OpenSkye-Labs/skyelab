//! Acceptance (QA #10): onboarding is never covered. Opening it hides the terminal drawer and the right panel, which
//! otherwise cut its card off and push Continue out of reach.

use super::worktree_thread_test::{fixture, open_thread};
use super::*;
use gpui_component::dock::DockPlacement;

#[gpui::test]
fn opening_onboarding_hides_the_drawer_and_the_right_panel(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.dock.update(cx, |dock, cx| {
            for at in [DockPlacement::Bottom, DockPlacement::Right] {
                if !dock.is_dock_open(at) {
                    dock.toggle_dock(at, window, cx);
                }
            }
        })
    });
    cx.run_until_parked();
    cx.dispatch_action(OpenOnboarding);
    cx.run_until_parked();
    let (bottom, right) = ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        (
            dock.is_dock_open(DockPlacement::Bottom),
            dock.is_dock_open(DockPlacement::Right),
        )
    });
    assert!(!bottom, "the terminal drawer is hidden");
    assert!(!right, "the right panel is hidden");
}
