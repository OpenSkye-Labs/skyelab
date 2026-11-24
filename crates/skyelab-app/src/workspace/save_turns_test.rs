//! A child's `Result` whose saves first fail and later succeed still ends its turn exactly once, and the child's
//! slot under the cap frees once the store takes the idle status.
#![cfg(unix)]

use super::delegation_test::{host_call, open, rig_with, start, thread_id};
use super::thread_model::Record;
use super::*;
use gpui::VisualTestContext;
use skyelab_agent::Event;
use skyelab_agent::mcp::{Caller, Tool};
use std::sync::atomic::Ordering;

use crate::test_wait::wait_ticking;

#[gpui::test]
fn a_late_saved_result_ends_its_turn_once_and_frees_the_slot(cx: &mut gpui::TestAppContext) {
    let (rig, accounts, config) =
        rig_with(|home| Store::open(&home.join("store.db")).expect("store file"));
    let db =
        rusqlite::Connection::open(rig._home.path().join("store.db")).expect("second connection");
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |p: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: p.into(),
        worktree: false,
        request_id: None,
    };

    // From here the store refuses agent rows and the idle status: every child's Result is held.
    db.execute_batch(
        "CREATE TRIGGER fail_agent BEFORE INSERT ON events WHEN NEW.kind = 'agent'
           BEGIN SELECT RAISE(ABORT, 'injected failure'); END;
         CREATE TRIGGER fail_idle BEFORE UPDATE ON threads WHEN NEW.status = 'idle'
           BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )
    .expect("inject failure");
    for n in 0..4 {
        host_call(cx, caller(), start_tool(&format!("child {n}"))).expect("under the limit");
    }
    let kids: Vec<i64> = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")
        .iter()
        .map(|t| t.id)
        .collect();
    let view = |cx: &mut VisualTestContext, id: i64| {
        ws.read_with(cx, |ws, cx| ws.open_view(id, cx))
            .expect("child tab open")
    };
    wait_ticking(cx, "every child's result shown but unsaved", |cx| {
        kids.iter().all(|id| {
            view(cx, *id).read_with(cx, |v, _| {
                v.turns.load(Ordering::SeqCst) == 0 && v.unsaved_events > 0
            })
        })
    });
    let err =
        host_call(cx, caller(), start_tool("child 5")).expect_err("the slots are still taken");
    assert!(err.contains("children"), "{err}");

    db.execute_batch("DROP TRIGGER fail_agent; DROP TRIGGER fail_idle;")
        .expect("heal");
    wait_ticking(cx, "every held write saved", |cx| {
        kids.iter()
            .all(|id| view(cx, *id).read_with(cx, |v, _| v.unsaved_events == 0))
    });
    for id in &kids {
        let v = view(cx, *id);
        assert_eq!(
            v.read_with(cx, |v, _| v.turns.load(Ordering::SeqCst)),
            0,
            "turn ended once"
        );
        let s = store.lock().expect("lock");
        assert_eq!(s.thread(*id).expect("read").expect("row").status, "idle");
        let results = s
            .events_since(*id, 0)
            .expect("events")
            .iter()
            .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
            .filter(|r| matches!(r, Record::Agent(Event::Result { .. })))
            .count();
        assert_eq!(results, 1, "the held result is stored once");
    }
    host_call(cx, caller(), start_tool("child 5")).expect("the idle children freed their slots");
}
