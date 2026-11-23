//! Acceptance 5: one scheduler with two windows open. A due task starts one thread, and the window that closes hands
//! the scheduler to the other.

use super::super::multi_window_test::in_window;
use super::super::threads_test::wait_until;
use super::super::windows::{self, NewWindow, window_handles};
use super::test_util::{HOUR, NOW, open, rig, runs, task, tick};
use gpui::TestAppContext;

#[gpui::test]
fn two_windows_start_one_thread(cx: &mut TestAppContext) {
    let rig = rig(true);
    let first = task(&rig, "every 1h", NOW - 1);
    let (a, cx) = open(&rig, cx);
    // As at launch: the first window takes the app-wide work, and the next one opened finds it taken.
    let a_window = cx.update(|window, _| window.window_handle());
    in_window(a_window, &a, cx, |ws, window, cx| ws.claim_hub(window, cx));
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let handles = cx.update(|_, cx| window_handles(cx));
    assert_eq!(handles.len(), 2, "a second window");
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    in_window(handles[1], &b, cx, |ws, window, cx| {
        ws.claim_hub(window, cx)
    });
    let hub = cx.update(|_, cx| windows::hub_workspace(cx).map(|h| h.0.window_id()));
    assert_eq!(hub, Some(a_window.window_id()), "the first window keeps it");

    tick(cx);
    wait_until(cx, "the run", |_| runs(&rig, first.id).len() == 1);
    tick(cx);
    cx.run_until_parked();
    assert_eq!(runs(&rig, first.id).len(), 1, "one thread for two windows");
    let shown_in_a = cx.update(|_, cx| a.read(cx).open_thread_ids(cx));
    assert_eq!(
        shown_in_a,
        [runs(&rig, first.id)[0].id],
        "started on the hub window"
    );

    // The hub window closes: the other one runs the next task.
    cx.update(|window, _| window.remove_window());
    let mut bcx = gpui::VisualTestContext::from_window(handles[1], cx);
    bcx.run_until_parked();
    let hub = bcx.update(|_, cx| windows::hub_workspace(cx).map(|h| h.0.window_id()));
    assert_eq!(
        hub,
        Some(handles[1].window_id()),
        "handed to the window left"
    );
    let second = task(&rig, "every 1h", NOW + HOUR - 1);
    bcx.update(|_, cx| super::set_now(NOW + HOUR, cx));
    tick(&mut bcx);
    wait_until(&mut bcx, "the next run", |_| {
        runs(&rig, second.id).len() == 1
    });
}
