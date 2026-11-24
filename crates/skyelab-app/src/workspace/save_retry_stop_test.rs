//! Held events survive the agent's stream ending, are retried on a timer with nothing else happening, and a
//! follow-up prompt is stored after the replies held before it.
#![cfg(unix)] // the fake agent is a shell script

use super::thread::{Stop, ThreadView};
use super::thread_model::{Item, Record};
use super::threads_test::{
    Fixture, fixture, open_workspace, project_thread, setup, stored_records, type_and_send,
};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Event;
use skyelab_store::Store;
use std::time::Duration;

use crate::test_wait::wait_ticking;

fn failing(f: &mut Fixture) -> rusqlite::Connection {
    let path = f._home.path().join("store.db");
    f.store = Arc::new(Mutex::new(Store::open(&path).expect("store file")));
    rusqlite::Connection::open(path).expect("second connection")
}

const FAIL: &str = "CREATE TRIGGER fail_agent BEFORE INSERT ON events WHEN NEW.kind = 'agent'
                    BEGIN SELECT RAISE(ABORT, 'injected failure'); END;";

fn first_turn_unsaved(thread: &Entity<ThreadView>, cx: &mut VisualTestContext) {
    type_and_send(thread, cx);
    wait_ticking(cx, "the first turn shown, unsaved", |cx| {
        thread.read_with(cx, |t, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. })) && t.unsaved_events > 0
        })
    });
}

#[gpui::test]
fn held_events_survive_a_stop_and_are_retried_on_a_timer(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = failing(&mut f);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    db.execute_batch(FAIL).expect("inject failure");
    first_turn_unsaved(&thread, cx);
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_ticking(cx, "stopped", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none())
    });
    for _ in 0..40 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert!(
        thread.read_with(cx, |t, _| t.unsaved_events > 0),
        "still held after the stream ended"
    );

    db.execute_batch("DROP TRIGGER fail_agent").expect("heal");
    wait_ticking(cx, "the held events saved with nothing sent", |cx| {
        thread.read_with(cx, |t, _| t.unsaved_events == 0)
    });
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    let results = stored_records(&f.store, id)
        .into_iter()
        .filter(|r| matches!(r, Record::Agent(Event::Result { .. })))
        .count();
    assert_eq!(results, 1, "held events from the stopped run were dropped");
}

#[gpui::test]
fn a_follow_up_is_stored_after_the_replies_held_before_it(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let db = failing(&mut f);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    db.execute_batch(FAIL).expect("inject failure");
    first_turn_unsaved(&thread, cx);
    type_and_send(&thread, cx); // a follow-up while the first turn's replies are still held
    db.execute_batch("DROP TRIGGER fail_agent").expect("heal");
    wait_ticking(cx, "everything saved", |cx| {
        thread.read_with(cx, |t, _| t.unsaved_events == 0)
    });

    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    let rows = stored_records(&f.store, id);
    let result = rows
        .iter()
        .position(|r| matches!(r, Record::Agent(Event::Result { .. })))
        .expect("result stored");
    let prompts: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| matches!(r, Record::Prompt(_)))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(prompts.len(), 2, "{rows:?}");
    assert!(
        prompts[1] > result,
        "the follow-up is stored after the reply it answers: {rows:?}"
    );
    let status = f
        .store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row")
        .status;
    assert_eq!(
        status, "running",
        "a held result saved late doesn't mark the follow-up's turn idle"
    );
}
