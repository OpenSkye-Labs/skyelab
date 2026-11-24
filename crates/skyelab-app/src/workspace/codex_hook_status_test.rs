//! While Codex runs one of the account's hooks, the thread's working row names it; once it finishes the row is
//! back to "Working for …" and the turn goes on. Hook lines are shown, never stored.

use super::claude_folded_turns_test::{one_view, row, wait_until};
use super::thread_model::Record;
use gpui::TestAppContext;
use skyelab_agent::Event;
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn a_running_codex_hook_shows_in_the_working_row(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("hook please", window, cx))
        .expect("started");
    wait_until(cx, "the hook in the working row", |cx| {
        one.view.read_with(cx, |v, _| {
            v.running_hook().is_some_and(|(line, _)| {
                line.starts_with("Running hook sessionStart (Reporting status to Orca)…")
            })
        })
    });
    // The fake holds the hook until this file is there.
    std::fs::write(one.dir.join(".fake-hook-done"), "").expect("go-ahead");
    wait_until(cx, "the turn's result", |cx| {
        row(&one, cx).is_some_and(|(status, n)| n == 1 && status == "idle")
    });
    assert!(
        one.view.read_with(cx, |v, _| v.running_hook().is_none()),
        "the hook finished"
    );
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().expect("row").id);
    let stored = one
        .store
        .lock()
        .expect("lock")
        .events_since(id, 0)
        .expect("events")
        .iter()
        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
        .filter(|r| matches!(r, Record::Agent(Event::Hook { .. })))
        .count();
    assert_eq!(stored, 0, "hook lines are never stored");
}

#[gpui::test]
fn stopping_during_a_hook_clears_it(cx: &mut TestAppContext) {
    let bin =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-agent/tests/fixtures/fake-codex.sh");
    let (one, cx) = one_view(cx, Provider::Codex, bin, None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("hook please", window, cx))
        .expect("started");
    wait_until(cx, "the hook to run", |cx| {
        one.view.read_with(cx, |v, _| !v.hooks.is_empty())
    });
    one.view
        .update_in(cx, |v, window, cx| v.stop(&super::thread::Stop, window, cx));
    // When the kill lands, not when the old agent's output drains: a send right after starts a new launch.
    wait_until(cx, "the agent stopped with no hook left", |cx| {
        one.view
            .read_with(cx, |v, _| v.session.is_none() && v.hooks.is_empty())
    });
}
