//! An agent event whose store write fails is still shown, held and retried, never dropped, and the thread says
//! some events weren't saved until they are.
#![cfg(unix)] // the fake agent is a shell script

use super::thread::ThreadView;
use super::thread_model::{Item, Record};
use super::threads_test::{
    Fixture, fixture, open_workspace, project_thread, setup, stored_records, type_and_send,
};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Event;
use skyelab_store::Store;
use std::time::Duration;

/// Like `wait_until`, but moves the test clock too, so retry timers fire.
fn wait_ticking(
    cx: &mut VisualTestContext,
    what: &str,
    mut done: impl FnMut(&mut VisualTestContext) -> bool,
) {
    for _ in 0..1000 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

fn agent_records(f: &Fixture, thread: i64) -> Vec<Event> {
    stored_records(&f.store, thread)
        .into_iter()
        .filter_map(|r| match r {
            Record::Agent(e) => Some(e),
            Record::Prompt(_) | Record::Steer(_) => None,
        })
        .collect()
}

fn db(f: &Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open(f._home.path().join("store.db")).expect("second connection")
}

#[gpui::test]
fn failed_event_writes_are_held_retried_and_reported(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    f.store = Arc::new(Mutex::new(
        Store::open(&f._home.path().join("store.db")).expect("store file"),
    ));
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);

    // Agent events can't be written from now on; prompts still can.
    db(&f)
        .execute_batch(
            "CREATE TRIGGER fail_agent BEFORE INSERT ON events WHEN NEW.kind = 'agent'
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .expect("inject failure");
    type_and_send(&thread, cx);
    wait_ticking(cx, "the result shown while unsaved", |cx| {
        thread.read_with(cx, |t: &ThreadView, _| {
            t.items.iter().any(|i| matches!(i, Item::Done { .. })) && t.unsaved_events > 0
        })
    });
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    assert!(
        agent_records(&f, id).is_empty(),
        "the injected failure holds"
    );
    assert!(
        cx.debug_bounds("unsaved-events").is_some(),
        "the thread says some events weren't saved"
    );

    // Writes work again: the next event saves everything held first, in order, once each.
    db(&f)
        .execute_batch("DROP TRIGGER fail_agent")
        .expect("heal");
    type_and_send(&thread, cx);
    wait_ticking(cx, "every held event saved", |cx| {
        thread.read_with(cx, |t, _| t.unsaved_events == 0)
    });
    let saved = agent_records(&f, id);
    assert!(
        matches!(
            &saved[..],
            [Event::Mcp { launch: true, .. }, Event::Init { .. }, ..]
        ),
        "{saved:?}"
    );
    assert_eq!(
        saved
            .iter()
            .filter(|e| matches!(e, Event::Result { .. }))
            .count(),
        1,
        "{saved:?}"
    );
    let reports = saved
        .iter()
        .filter(|e| matches!(e, Event::Unknown(v) if v["type"] == "fake_env"))
        .count();
    assert_eq!(reports, 1, "no event saved twice");
    let shown = thread.read_with(cx, |t, _| t.items.len());
    assert!(shown > 0);
    let row = f
        .store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("row");
    assert_eq!(
        row.agent_session_id.as_deref(),
        Some("fake-sess-1"),
        "the held Init's side effect ran too"
    );
    assert!(
        cx.debug_bounds("unsaved-events").is_none(),
        "warning cleared"
    );
}
