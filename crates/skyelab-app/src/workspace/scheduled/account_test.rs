//! Acceptance 3: a run uses the folder's account as resolved when it runs, only if it is the one the task was saved
//! under. Unbound, rebound or unreadable: no thread, and the task pauses with the reason.

use super::super::threads_test::wait_until;
use super::test_util::{HOUR, NOW, open, read, rebind, rig, runs, task, tick};
use gpui::TestAppContext;

#[gpui::test]
fn a_due_task_runs_under_the_saved_account(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 1);
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the scheduled thread's row", |_| {
        runs(&rig, t.id).len() == 1
    });
    let run = &runs(&rig, t.id)[0];
    assert_eq!(run.account_id, "acme");
    let after = read(&rig.f.store, t.id);
    assert_eq!(after.last_status.as_deref(), Some("started"));
    assert_eq!(after.next_run_at, Some(NOW - 1 + HOUR), "keeps its beat");
    assert!(after.enabled);
}

fn paused_with(rebound_to: Option<&str>, garbage: bool, cx: &mut TestAppContext) -> String {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 1);
    let (_ws, cx) = open(&rig, cx);
    // The workspace loaded "acme" at open; the change lands on disk after that, as an edit elsewhere would.
    rebind(&rig, rebound_to);
    if garbage {
        std::fs::write(&rig.f.accounts_path, "not = [toml").expect("write");
    }
    tick(cx);
    cx.run_until_parked();
    assert!(runs(&rig, t.id).is_empty(), "no thread started");
    let after = read(&rig.f.store, t.id);
    assert!(!after.enabled, "the task paused");
    assert_eq!(after.last_status.as_deref(), Some("paused"));
    // Paused, it doesn't run again on the next tick either.
    tick(cx);
    cx.run_until_parked();
    assert!(runs(&rig, t.id).is_empty());
    after.last_error.expect("a reason")
}

#[gpui::test]
fn a_rebound_folder_pauses_the_task(cx: &mut TestAppContext) {
    let why = paused_with(Some("home"), false, cx);
    assert!(why.contains("home") && why.contains("acme"), "{why}");
}

#[gpui::test]
fn an_unbound_folder_pauses_the_task(cx: &mut TestAppContext) {
    let why = paused_with(None, false, cx);
    assert!(why.contains("isn't bound"), "{why}");
}

#[gpui::test]
fn an_unreadable_accounts_file_binds_nothing(cx: &mut TestAppContext) {
    let why = paused_with(Some("acme"), true, cx);
    assert!(why.contains("isn't bound"), "{why}");
}
