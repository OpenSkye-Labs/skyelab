//! The thread's permission mode reaches the agent at launch and while it runs, and is kept on the row. A new thread
//! starts in Full access; switching to it from another mode needs a confirm first.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::Record;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::{Event, Mode};
use skyelab_core::Provider;
use std::path::Path;

fn fake() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

/// What the fake echoed or reported, as JSON.
fn unknowns(one: &One, cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let Some(id) = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
    else {
        return Vec::new();
    };
    let s = one.store.lock().expect("lock");
    s.events_since(id, 0)
        .expect("events")
        .iter()
        .filter_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(Event::Unknown(v))) => Some(v),
            _ => None,
        })
        .collect()
}

fn args(one: &One, cx: &mut VisualTestContext) -> String {
    let mut args = String::new();
    wait_until(cx, "the fake's env report", |cx| {
        if let Some(v) = unknowns(one, cx)
            .into_iter()
            .find(|v| v["type"] == "fake_env")
        {
            args = v["args"].as_str().unwrap_or_default().to_owned();
            return true;
        }
        false
    });
    args
}

fn row_mode(one: &One, cx: &mut VisualTestContext) -> String {
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
        .expect("row");
    let s = one.store.lock().expect("lock");
    s.thread(id).expect("read").expect("row").permission_mode
}

#[gpui::test]
fn mode_is_passed_and_kept_on_the_row(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view.update(cx, |v, cx| v.set_mode(Mode::Plan, cx));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("hi", window, cx))
        .expect("started");
    let args = args(&one, cx);
    assert!(args.contains("--permission-prompt-tool stdio"), "{args}");
    assert!(args.contains("--permission-mode plan"), "{args}");
    assert_eq!(row_mode(&one, cx), "plan");

    // A change while the agent runs is sent to it, and stored.
    one.view
        .update(cx, |v, cx| v.set_mode(Mode::AcceptEdits, cx));
    wait_until(cx, "the mode change to reach the agent", |cx| {
        unknowns(&one, cx).iter().any(|v| {
            v["type"] == "control_request"
                && v["request"]["subtype"] == "set_permission_mode"
                && v["request"]["mode"] == "acceptEdits"
        })
    });
    assert_eq!(row_mode(&one, cx), "accept_edits");
}

#[gpui::test]
fn a_new_thread_starts_in_full_access(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view.read_with(cx, |v, _| {
        assert_eq!(v.mode, Mode::Bypass);
        assert!(!v.confirm_bypass, "a default asks no confirm");
    });
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("permit the deploy", window, cx))
        .expect("started");
    let args = args(&one, cx);
    assert!(
        args.contains("--permission-mode bypassPermissions --allow-dangerously-skip-permissions"),
        "{args}"
    );
    wait_until(cx, "the command to run unasked", |cx| {
        unknowns(&one, cx)
            .iter()
            .any(|v| v["type"] == "fake_answer" && v["reply"] == "bypassed")
    });
    one.view.read_with(cx, |v, _| {
        let asked = v
            .items
            .iter()
            .any(|i| matches!(i, super::thread_model::Item::Approval(_)));
        assert!(!asked, "nothing asked");
    });
    wait_until(cx, "the row to keep it", |cx| row_mode(&one, cx) == "bypass");
}

#[gpui::test]
fn full_access_needs_a_confirm(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view.update(cx, |v, cx| v.set_mode(Mode::Default, cx)); // leaving Full access needs no confirm
    let mode =
        |cx: &mut VisualTestContext| one.view.read_with(cx, |v, _| (v.mode, v.confirm_bypass));
    one.view.update(cx, |v, cx| v.set_mode(Mode::Bypass, cx));
    assert_eq!(mode(cx), (Mode::Default, true), "asked, not on yet");
    one.view.update(cx, |v, cx| v.confirm_mode(false, cx));
    assert_eq!(mode(cx), (Mode::Default, false), "turned down");
    // A confirm with nothing asked does nothing.
    one.view.update(cx, |v, cx| v.confirm_mode(true, cx));
    assert_eq!(mode(cx), (Mode::Default, false));
    one.view.update(cx, |v, cx| v.set_mode(Mode::Bypass, cx));
    one.view.update(cx, |v, cx| v.confirm_mode(true, cx));
    assert_eq!(mode(cx), (Mode::Bypass, false));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("hi", window, cx))
        .expect("started");
    let args = args(&one, cx);
    assert!(
        args.contains("--permission-mode bypassPermissions --allow-dangerously-skip-permissions"),
        "{args}"
    );
}

#[gpui::test]
fn a_mode_picked_while_starting_reaches_the_agent_and_the_row(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view
        .update_in(cx, |v, window, cx| {
            let sent = v.submit_remote("hi", window, cx);
            // Before the row or the agent exists.
            v.set_mode(Mode::Plan, cx);
            sent
        })
        .expect("started");
    wait_until(cx, "the mode change to reach the agent", |cx| {
        unknowns(&one, cx)
            .iter()
            .any(|v| v["type"] == "control_request" && v["request"]["mode"] == "plan")
    });
    wait_until(cx, "the row to keep it", |cx| row_mode(&one, cx) == "plan");
}

#[gpui::test]
fn full_access_on_a_running_claude_waits_for_a_restart(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view.update(cx, |v, cx| v.set_mode(Mode::Default, cx));
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("hi", window, cx))
        .expect("started");
    let _ = args(&one, cx);
    one.view.update(cx, |v, cx| {
        v.set_mode(Mode::Bypass, cx);
        v.confirm_mode(true, cx);
    });
    wait_until(cx, "the error saying when it applies", |cx| {
        one.view.read_with(cx, |v, _| {
            v.error
                .as_ref()
                .is_some_and(|e| e.contains("applies when this agent starts again"))
        })
    });
    assert!(
        !unknowns(&one, cx)
            .iter()
            .any(|v| v["request"]["mode"] == "bypassPermissions"),
        "not sent to an agent that would refuse it"
    );
    wait_until(cx, "the row to keep the pick for the next start", |cx| {
        row_mode(&one, cx) == "bypass"
    });
}
