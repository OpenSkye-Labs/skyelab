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

/// The idle write of the thread's last turn, held by a store that refuses it.
fn hold_idle(home: &Path) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(home.join("store.db")).expect("second connection");
    db.execute_batch(
        "CREATE TRIGGER fail_idle BEFORE UPDATE ON threads WHEN NEW.status = 'idle'
           BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )
    .expect("inject failure");
    db
}

fn status(store: &SharedStore, id: i64) -> String {
    store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row")
        .status
}

/// Closing the tab (whose last try flushes what is held) and quitting both leave `stopped`, whichever lands first.
#[gpui::test]
fn a_held_idle_does_not_overwrite_a_closed_or_quit_thread(cx: &mut gpui::TestAppContext) {
    let (rig, accounts, config) =
        rig_with(|home| Store::open(&home.join("store.db")).expect("store file"));
    let db = hold_idle(rig._home.path());
    let (ws, vcx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let held = |v: &Entity<ThreadView>, cx: &mut VisualTestContext| {
        v.read_with(cx, |v, _| {
            v.turns.load(Ordering::SeqCst) == 0 && v.unsaved_events > 0
        })
    };

    // Closed: the tab goes while its idle is held; the store heals right after.
    let closed = start(&ws, rig.work_a, "plain work", vcx);
    let closed_id = thread_id(&closed, vcx);
    tick(vcx, "result shown, idle held", |cx| held(&closed, cx));
    ws.update_in(vcx, |ws, window, cx| {
        let tab = closed.clone();
        ws.dock
            .update(cx, |dock, cx| dock.remove_panel(tab, window, cx));
    });
    drop(closed);
    tick(vcx, "the closed thread stopped", |_| {
        status(&store, closed_id) == "stopped"
    });
    db.execute_batch("DROP TRIGGER fail_idle;").expect("heal");
    for _ in 0..20 {
        vcx.executor().advance_clock(Duration::from_millis(500));
        vcx.run_until_parked();
    }
    assert_eq!(status(&store, closed_id), "stopped", "closed tab");

    // Quit: the idle is still held in the view, and the store heals before its next retry.
    let _db = hold_idle(rig._home.path());
    let quit = start(&ws, rig.work_a, "plain work", vcx);
    let quit_id = thread_id(&quit, vcx);
    tick(vcx, "result shown, idle held", |cx| held(&quit, cx));
    db.execute_batch("DROP TRIGGER fail_idle;").expect("heal");
    cx.quit();
    assert_eq!(status(&store, quit_id), "stopped", "quit");
}
