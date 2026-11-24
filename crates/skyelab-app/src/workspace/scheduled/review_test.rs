//! Review fixes: the hub reads projects from the store, a task the cap held isn't "missed", a task keeps one live
//! agent, and a run waiting on a permission answer says so.

use super::super::lock;
use super::super::threads_test::wait_until;
use super::test_util::{NOW, open, read, rig, runs, set_settings, stored_run, task, tick};
use gpui::TestAppContext;
use skyelab_core::ScheduledSettings;
use skyelab_store::scheduled::ScheduledTask;

const MINUTE: i64 = 60_000;

#[gpui::test]
fn a_project_added_after_the_window_loaded_still_runs(cx: &mut TestAppContext) {
    let rig = rig(true);
    let (_ws, cx) = open(&rig, cx);
    cx.run_until_parked();
    // Added as another window would: in the store, not in this window's list.
    let dir = rig.f.project_dir.join("svc");
    std::fs::create_dir_all(&dir).expect("dir");
    let project = lock(&rig.f.store)
        .expect("store")
        .create_project("svc", &dir)
        .expect("project");
    let mut t = ScheduledTask {
        project_id: project.id,
        title: "svc".into(),
        prompt: "scheduled hello".into(),
        schedule: "every 1h".into(),
        permission_mode: "default".into(),
        account_id: "acme".into(),
        enabled: true,
        next_run_at: Some(NOW - 1),
        ..Default::default()
    };
    t.id = lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    tick(cx);
    cx.run_until_parked();
    let after = read(&rig.f.store, t.id);
    assert!(after.enabled, "paused: {:?}", after.last_error);
    assert_eq!(after.last_status.as_deref(), Some("started"));
}

#[gpui::test]
fn a_task_the_cap_held_is_not_skipped_as_missed(cx: &mut TestAppContext) {
    let rig = rig(true);
    let busy = task(&rig, "every 1h", NOW + 60 * MINUTE);
    let running = stored_run(&rig, busy.id, "running");
    let waiting = task(&rig, "every 1h", NOW - 1);
    let (_ws, cx) = open(&rig, cx);
    set_settings(
        ScheduledSettings {
            skip_missed: true,
            max_concurrent: 1,
        },
        cx,
    );
    // The app runs (ticks keep coming) while the cap holds it for twelve minutes.
    for m in 0..=12 {
        cx.update(|_, cx| super::set_now(NOW + m * MINUTE, cx));
        tick(cx);
    }
    cx.run_until_parked();
    assert_eq!(
        read(&rig.f.store, waiting.id).last_status,
        None,
        "still held"
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

#[gpui::test]
fn a_new_run_closes_the_tasks_finished_one(cx: &mut TestAppContext) {
    let rig = rig(true);
    let t = task(&rig, "every 1m", NOW - 1);
    let (ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the first run ends", |_| {
        runs(&rig, t.id).first().is_some_and(|r| r.status == "idle")
    });
    cx.update(|_, cx| super::set_now(NOW + MINUTE, cx));
    tick(cx);
    wait_until(cx, "the second run", |_| runs(&rig, t.id).len() == 2);
    cx.run_until_parked();
    let open_ids = cx.update(|_, cx| ws.read(cx).open_thread_ids(cx));
    assert_eq!(
        open_ids,
        [runs(&rig, t.id)[1].id],
        "one live agent per task"
    );
}

#[gpui::test]
fn a_run_waiting_on_a_permission_answer_says_so(cx: &mut TestAppContext) {
    let rig = rig(true);
    let mut t = task(&rig, "every 1h", NOW - 1);
    t.prompt = "permit the deploy".into();
    lock(&rig.f.store)
        .expect("store")
        .save_scheduled_task(&t)
        .expect("save");
    let (ws, cx) = open(&rig, cx);
    tick(cx);
    wait_until(cx, "the run's row", |_| !runs(&rig, t.id).is_empty());
    let id = runs(&rig, t.id)[0].id;
    let asking = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.open_threads.iter().any(|v| {
                let v = v.read(cx);
                v.thread.as_ref().is_some_and(|t| t.id == id)
                    && super::super::thread_rows::awaiting_approval(v)
            })
        })
    };
    wait_until(cx, "the run asks", asking);
    // The fake's replayed session ends a turn first ("Thread completed"); the ask is a second notice.
    let note = format!("thread {id}");
    wait_until(cx, "an Approval needed notice", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.notices.iter().filter(|n| n.as_ref() == note).count() == 2
        })
    });
    assert!(asking(cx), "still waiting on the answer");
}
