//! A held idle write never overwrites a later Stop.
#![cfg(unix)]

use super::delegation_test::{open, rig_with, start, thread_id};
use super::thread::Stop;
use super::*;
use gpui::VisualTestContext;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn tick(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1500 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

#[gpui::test]
fn a_held_idle_does_not_overwrite_stopped(cx: &mut gpui::TestAppContext) {
    let (rig, accounts, config) =
        rig_with(|home| Store::open(&home.join("store.db")).expect("store file"));
    let db =
        rusqlite::Connection::open(rig._home.path().join("store.db")).expect("second connection");
    db.execute_batch(
        "CREATE TRIGGER fail_idle BEFORE UPDATE ON threads WHEN NEW.status = 'idle'
           BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )
    .expect("inject failure");
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let view = start(&ws, rig.work_a, "plain work", cx);
    let id = thread_id(&view, cx);
    tick(cx, "result shown, idle held", |cx| {
        view.read_with(cx, |v, _| {
            v.turns.load(Ordering::SeqCst) == 0 && v.unsaved_events > 0
        })
    });
    view.update_in(cx, |v, w, cx| v.stop(&Stop, w, cx));
    tick(cx, "stopped stored", |_| {
        store
            .lock()
            .expect("lock")
            .thread(id)
            .expect("read")
            .expect("row")
            .status
            == "stopped"
    });
    db.execute_batch("DROP TRIGGER fail_idle;").expect("heal");
    tick(cx, "held writes drained", |cx| {
        view.read_with(cx, |v, _| v.unsaved_events == 0)
    });
    let status = store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row")
        .status;
    assert_eq!(status, "stopped", "STALE_IDLE: got {status}");
}
