//! Acceptance (QA #10): onboarding is never covered. Opening it hides the terminal drawer and the right panel, which
//! otherwise cut its card off and push Continue out of reach.

use super::worktree_thread_test::{fixture, open_thread};
use super::*;
use crate::ui::dock::DockPlacement;

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

#[gpui::test]
fn the_pull_requests_page_puts_the_docks_away_and_a_thread_brings_them_back(
    cx: &mut gpui::TestAppContext,
) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    let docks = |ws: &Entity<Workspace>, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            let dock = ws.dock.read(cx);
            (
                dock.is_dock_open(DockPlacement::Right),
                dock.is_dock_open(DockPlacement::Bottom),
            )
        })
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.dock.update(cx, |dock, cx| {
            if !dock.is_dock_open(DockPlacement::Right) {
                dock.toggle_dock(DockPlacement::Right, window, cx);
            }
            if dock.is_dock_open(DockPlacement::Bottom) {
                dock.toggle_dock(DockPlacement::Bottom, window, cx);
            }
        })
    });
    cx.run_until_parked();
    assert_eq!(docks(&ws, cx), (true, false), "precondition");
    cx.dispatch_action(super::gh_pages::OpenPullRequests);
    cx.run_until_parked();
    assert_eq!(docks(&ws, cx), (false, false), "the page gets the window's width");
    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(docks(&ws, cx), (true, false), "back on the thread: only what was open returns");
}

#[gpui::test]
fn a_new_thread_in_the_folder_takes_over_its_changes_tab_title(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    let changes = ws.update_in(cx, |ws, window, cx| ws.open_changes(&thread, window, cx));
    cx.run_until_parked();
    let before = changes.read_with(cx, |c, _| c.title());
    assert_eq!(before, format!("Changes · {}", thread.read_with(cx, |t, _| t.title())));
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let shown = ws.read_with(cx, |ws, _| ws.current_thread()).expect("a new thread page");
    assert_ne!(shown, thread, "precondition: another thread shows");
    assert_eq!(
        changes.read_with(cx, |c, _| c.title()).as_ref(),
        "Changes · New thread",
        "the tab names the thread shown, not the one before"
    );
}
