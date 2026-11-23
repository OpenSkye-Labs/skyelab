//! A Codex thread whose resumed thread is gone says so and runs the prompt again in a new thread, as Claude does:
//! codex stays up after refusing `thread/resume`, so the view ends it and starts a fresh one.

use super::claude_folded_turns_test::{one_view, row, wait_until};
use super::thread::Stop;
use super::thread_model::{Item, Record, SESSION_GONE_NOTE};
use gpui::TestAppContext;
use skyelab_agent::Event;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn a_gone_codex_thread_continues_in_a_new_one(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("first", window, cx))
        .expect("started");
    wait_until(cx, "the first result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 1 && status == "idle")
    });
    one.view
        .update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the session to stop", |cx| {
        one.view
            .read_with(cx, |v, _| v.session.is_none() && !v.read_only)
    });
    // From now on the fake has no rollout for thr_fake.
    std::fs::write(one.dir.join(".fake-session-gone"), "").expect("marker");
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("again", window, cx))
        .expect("resumed");
    wait_until(cx, "the rerun's result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 3 && status == "idle")
    });
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().expect("row").id);
    let records: Vec<Record> = one
        .store
        .lock()
        .expect("lock")
        .events_since(id, 0)
        .expect("events")
        .iter()
        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
        .collect();
    let starts = records
        .iter()
        .filter(|r| matches!(r, Record::Agent(Event::Unknown(v)) if v["method"] == "fake/env"))
        .count();
    assert_eq!(starts, 2, "the first thread and the rerun's new one");
    let agains = records
        .iter()
        .filter(|r| matches!(r, Record::Prompt(p) if p == "again"))
        .count();
    assert_eq!(agains, 1, "the prompt is stored once");
    assert!(
        records
            .iter()
            .any(|r| matches!(r, Record::Agent(Event::SessionGone))),
        "the gone thread is recorded"
    );
    one.view.read_with(cx, |v, _| {
        assert!(
            v.items
                .iter()
                .any(|i| matches!(i, Item::Notice { text, .. } if text == SESSION_GONE_NOTE)),
            "the thread says it continued in a new one: {:?}",
            v.items
        );
        assert!(v.session.is_some(), "the new thread runs");
        assert!(v.error.is_none(), "{:?}", v.error);
    });
}
