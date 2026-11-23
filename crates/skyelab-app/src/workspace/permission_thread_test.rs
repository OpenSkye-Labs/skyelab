//! A permission request shows in the thread, the agent waits for it, and the answer given there reaches the
//! agent. A crashed agent's stderr shows as an "Agent output" row.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::{Item, Record};
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::{Decision, Event};
use skyelab_core::Provider;
use std::path::Path;
use std::time::Duration;

fn fake() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

fn stored(one: &One, cx: &mut VisualTestContext) -> Vec<Event> {
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
            Ok(Record::Agent(ev)) => Some(ev),
            _ => None,
        })
        .collect()
}

/// The reply the fake reported getting, once it did.
fn answer_seen(one: &One, cx: &mut VisualTestContext) -> Option<serde_json::Value> {
    stored(one, cx).into_iter().find_map(|e| match e {
        Event::Unknown(v) if v["type"] == "fake_answer" => Some(v["reply"].clone()),
        _ => None,
    })
}

fn approval(one: &One, cx: &mut VisualTestContext) -> Option<(String, Option<Decision>)> {
    one.view.read_with(cx, |v, _| {
        v.items.iter().find_map(|i| match i {
            Item::Approval(a) => Some((a.summary.clone(), a.decision)),
            _ => None,
        })
    })
}

fn answer_in_thread(decision: Decision, cx: &mut TestAppContext) -> serde_json::Value {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view
        .update_in(cx, |v, window, cx| {
            v.submit_remote("permit the deploy", window, cx)
        })
        .expect("started");
    wait_until(cx, "the request in the thread", |cx| {
        approval(&one, cx).is_some()
    });
    assert_eq!(
        approval(&one, cx),
        Some(("make deploy".into(), None)),
        "shown with its command, waiting"
    );
    // Nothing reaches the agent until the user answers.
    for _ in 0..20 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        answer_seen(&one, cx).is_none(),
        "the agent went on unanswered"
    );
    let id = one.view.read_with(cx, |v, _| {
        v.items.iter().find_map(|i| match i {
            Item::Approval(a) => Some(a.id.clone()),
            _ => None,
        })
    });
    let id = id.expect("the request's id");
    one.view.update(cx, |v, cx| v.answer(&id, decision, cx));
    wait_until(cx, "the agent's report of the answer", |cx| {
        answer_seen(&one, cx).is_some()
    });
    wait_until(cx, "the row to say what was decided", |cx| {
        approval(&one, cx).is_some_and(|(_, d)| d == Some(decision))
    });
    answer_seen(&one, cx).expect("seen")
}

#[gpui::test]
fn allow_once_in_the_thread_reaches_the_agent(cx: &mut TestAppContext) {
    let reply = answer_in_thread(Decision::AllowOnce, cx);
    assert_eq!(
        reply["response"]["request_id"], "perm-1",
        "the CLI's own id goes back"
    );
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["command"],
        "make deploy"
    );
}

#[gpui::test]
fn deny_in_the_thread_reaches_the_agent(cx: &mut TestAppContext) {
    let reply = answer_in_thread(Decision::Deny, cx);
    assert_eq!(reply["response"]["response"]["behavior"], "deny");
}

#[gpui::test]
fn a_crashed_agent_shows_its_stderr(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("crash now", window, cx))
        .expect("started");
    wait_until(cx, "the agent output row", |cx| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().any(|i| {
                matches!(i, Item::Output { text, .. } if text.contains("fake-claude: lost the session"))
            })
        })
    });
    assert!(
        stored(&one, cx)
            .iter()
            .any(|e| matches!(e, Event::Stderr { text } if text.contains("lost the session"))),
        "stored, so a reopened or remote view shows it too"
    );
}
