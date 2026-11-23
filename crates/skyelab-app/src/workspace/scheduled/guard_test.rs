//! Acceptance 4: no overlapping runs of one task, and at most `max_concurrent` scheduled runs app-wide.

use super::super::lock;
use super::super::threads_test::wait_until;
use super::test_util::{HOUR, NOW, open, read, rig, runs, set_settings, stored_run, task, tick};
use gpui::TestAppContext;
use skyelab_core::ScheduledSettings;

#[gpui::test]
fn a_task_still_starting_is_not_run_again(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1m", NOW - 1);
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    // Before anything else runs: the first run's row may not exist yet, but its launch counts.
    cx.update(|_, cx| super::set_now(NOW + 2 * 60_000, cx));
    tick(cx);
    wait_until(cx, "the first run's row", |_| !runs(&rig, t.id).is_empty());
    cx.run_until_parked();
    assert_eq!(runs(&rig, t.id).len(), 1, "one run");
    let after = read(&rig.f.store, t.id);
    assert_eq!(after.last_status.as_deref(), Some("skipped"));
    assert!(after.last_error.expect("why").contains("still going"));
}

#[gpui::test]
fn a_stored_running_run_blocks_the_next(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 1);
    stored_run(&rig, t.id, "running");
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    cx.run_until_parked();
    assert_eq!(runs(&rig, t.id).len(), 1, "only the stored one");
    assert_eq!(
        read(&rig.f.store, t.id).next_run_at,
        Some(NOW - 1 + HOUR),
        "this slot is skipped"
    );
}

#[gpui::test]
fn a_full_app_holds_due_tasks_until_a_run_ends(cx: &mut TestAppContext) {
    let rig = rig(true);
    let busy = task(&rig, "every 1h", NOW + HOUR);
    let running = stored_run(&rig, busy.id, "running");
    let waiting = task(&rig, "every 1h", NOW - 1);
    let (_ws, cx) = open(&rig, cx);
    set_settings(
        ScheduledSettings {
            max_concurrent: 1,
            ..Default::default()
        },
        cx,
    );
    tick(cx);
    cx.run_until_parked();
    assert!(runs(&rig, waiting.id).is_empty(), "the cap holds it");
    assert_eq!(
        read(&rig.f.store, waiting.id).next_run_at,
        Some(NOW - 1),
        "still due"
    );
    assert_eq!(
        read(&rig.f.store, waiting.id).last_status,
        None,
        "not skipped"
    );

    lock(&rig.f.store)
        .expect("store")
        .set_thread_status(running, "idle")
        .expect("status");
    tick(cx);
    wait_until(cx, "the held task's run", |_| {
        runs(&rig, waiting.id).len() == 1
    });
}
