//! Acceptance 6 (runs): Full access runs only with the task's own confirm; without it the run is refused and the
//! task pauses. The editor's refusal is in `page_test`.

use super::super::lock;
use super::super::threads_test::wait_until;
use super::test_util::{NOW, open, read, rig, runs, task, tick};
use gpui::TestAppContext;

#[gpui::test]
fn full_access_without_a_confirm_never_runs(cx: &mut TestAppContext) {
    let rig = rig(true);
    let mut t = task(&rig, "every 1h", NOW - 1);
    t.permission_mode = "bypass".into();
    lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    cx.run_until_parked();
    assert!(runs(&rig, t.id).is_empty());
    let after = read(&rig.f.store, t.id);
    assert!(!after.enabled);
    assert!(after.last_error.expect("why").contains("Full access"));
}

#[gpui::test]
fn full_access_with_its_confirm_runs_in_full_access(cx: &mut TestAppContext) {
    let rig = rig(true);
    let mut t = task(&rig, "every 1h", NOW - 1);
    t.permission_mode = "bypass".into();
    t.bypass_confirmed = true;
    lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the run", |_| runs(&rig, t.id).len() == 1);
    assert_eq!(runs(&rig, t.id)[0].permission_mode, "bypass");
}

#[gpui::test]
fn a_task_saved_in_ask_runs_in_ask_though_new_threads_default_to_full_access(
    cx: &mut TestAppContext,
) {
    let rig = rig(true);
    let t = task(&rig, "every 1h", NOW - 1);
    assert_eq!(t.permission_mode, "default");
    lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    let (_ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the run", |_| runs(&rig, t.id).len() == 1);
    assert_eq!(runs(&rig, t.id)[0].permission_mode, "default");
}
