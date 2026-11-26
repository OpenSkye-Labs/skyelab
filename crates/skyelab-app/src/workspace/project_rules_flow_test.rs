//! "Allow for this project" with the fake agent's `permit` ask (`make deploy`): saving a rule allows the call, the
//! next matching ask is allowed by it without anyone answering and its row names the rule, and a rule never applies
//! under another account or in Plan mode.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::{Item, Record};
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::{Decision, Event, Mode};
use skyelab_core::Provider;
use skyelab_core::project_rules::{Kind, Rule};
use std::path::Path;
use std::time::Duration;

fn fake() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh")
}

/// The replies the fake reported getting, oldest first.
fn answers(one: &One, cx: &mut VisualTestContext) -> Vec<serde_json::Value> {
    let Some(id) = one.view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id)) else {
        return Vec::new();
    };
    let s = one.store.lock().expect("lock");
    s.events_since(id, 0)
        .expect("events")
        .iter()
        .filter_map(|e| match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(Event::Unknown(v))) if v["type"] == "fake_answer" => Some(v["reply"].clone()),
            _ => None,
        })
        .collect()
}

/// Every approval row: (id, decision, rule).
fn approvals(one: &One, cx: &mut VisualTestContext) -> Vec<(String, Option<Decision>, Option<String>)> {
    one.view.read_with(cx, |v, _| {
        v.items
            .iter()
            .filter_map(|i| match i {
                Item::Approval(a) => Some((a.id.clone(), a.decision, a.rule.clone())),
                _ => None,
            })
            .collect()
    })
}

fn project(one: &One) -> i64 {
    let s = one.store.lock().expect("lock");
    s.list_projects().expect("projects")[0].id
}

fn save_rule(one: &One, account: &str, rule: &Rule) {
    let p = project(one);
    one.store.lock().expect("lock").add_project_rule(p, account, rule).expect("save");
}

fn send(one: &One, prompt: &str, cx: &mut VisualTestContext) {
    one.view
        .update_in(cx, |v, window, cx| {
            // A new thread's Full access would skip the ask these tests are about; Plan stays Plan.
            if v.mode == Mode::Bypass {
                v.mode = Mode::Default;
            }
            v.submit_remote(prompt, window, cx)
        })
        .expect("sent");
}

fn prefix(p: &str) -> Rule {
    Rule { tool: "Bash".into(), kind: Kind::Prefix, pattern: p.into() }
}

/// Waits a moment, then checks the one ask is still waiting and the agent got nothing.
fn still_asks(one: &One, cx: &mut VisualTestContext) {
    wait_until(cx, "the ask", |cx| !approvals(one, cx).is_empty());
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(approvals(one, cx)[0].1, None, "still waits for the user");
    assert!(answers(one, cx).is_empty(), "nothing reached the agent");
}

#[gpui::test]
fn a_saved_rule_allows_this_call_and_the_next_one_by_itself(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    send(&one, "permit the deploy", cx);
    wait_until(cx, "the ask", |cx| !approvals(&one, cx).is_empty());
    let id = approvals(&one, cx)[0].0.clone();
    let pid = project(&one);

    // Refused, saving nothing: a rule that doesn't cover the waiting call, and a compound prefix.
    for bad in [prefix("make test"), prefix("make deploy;"), Rule { tool: "Edit".into(), ..prefix("x") }] {
        let r = one.view.update(cx, |v, cx| v.allow_by_new_rule(&id, bad.clone(), |_| {}, cx));
        assert!(r.is_err(), "{bad:?} was taken");
    }
    assert!(one.store.lock().expect("lock").project_rules(pid, "work").expect("list").is_empty());

    one.view
        .update(cx, |v, cx| v.allow_by_new_rule(&id, prefix("make deploy"), |_| {}, cx))
        .expect("saved");
    wait_until(cx, "the allow to reach the agent", |cx| answers(&one, cx).len() == 1);
    assert_eq!(answers(&one, cx)[0]["response"]["response"]["behavior"], "allow");
    let saved = one.store.lock().expect("lock").project_rules(pid, "work").expect("list");
    assert_eq!(saved.iter().map(|r| r.rule.clone()).collect::<Vec<_>>(), [prefix("make deploy")]);
    assert!(
        one.store.lock().expect("lock").project_rules(pid, "other").expect("list").is_empty(),
        "saved under the agent's account only"
    );

    // The next turn asks again: answered by the rule, with no one clicking, and the row says which rule.
    wait_until(cx, "the first turn to end", |cx| one.view.read_with(cx, |v, _| !v.turn_running()));
    send(&one, "permit it again", cx);
    wait_until(cx, "the second allow", |cx| answers(&one, cx).len() == 2);
    assert_eq!(answers(&one, cx)[1]["response"]["response"]["behavior"], "allow");
    wait_until(cx, "the row to name the rule", |cx| {
        approvals(&one, cx).get(1).is_some_and(|a| a.2.is_some())
    });
    let second = &approvals(&one, cx)[1];
    assert_eq!(second.1, Some(Decision::AllowOnce));
    assert_eq!(second.2.as_deref(), Some("Bash · starts with make deploy"));
    // Stored with the answer, so a reopened thread says the same.
    let thread = one.view.read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id)).expect("thread");
    let stored = one.store.lock().expect("lock").events_since(thread, 0).expect("events");
    assert!(stored.iter().any(|e| e.payload_json.contains("Bash · starts with make deploy")));
}

#[gpui::test]
fn another_accounts_rule_never_applies(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    save_rule(&one, "other", &prefix("make deploy"));
    send(&one, "permit the deploy", cx);
    still_asks(&one, cx);
}

#[gpui::test]
fn plan_mode_never_auto_allows(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    save_rule(&one, "work", &prefix("make deploy"));
    one.view.update(cx, |v, _| v.mode = Mode::Plan);
    send(&one, "permit the deploy", cx);
    still_asks(&one, cx);
}

#[gpui::test]
fn the_rule_that_covers_the_call_allows_it(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    save_rule(&one, "work", &prefix("make"));
    save_rule(&one, "work", &Rule { kind: Kind::Exact, ..prefix("make deploy --prod") });
    // "make" covers "make deploy": proves the lookup itself runs, against the exact one that doesn't.
    send(&one, "permit the deploy", cx);
    wait_until(cx, "allowed by the broad rule", |cx| answers(&one, cx).len() == 1);
    // The agent's reply can reach the thread before the row hears what answered it.
    wait_until(cx, "the row to name the rule", |cx| approvals(&one, cx)[0].2.is_some());
    assert_eq!(approvals(&one, cx)[0].2.as_deref(), Some("Bash · starts with make"));
}

/// A refusal about an ask (no rule can cover it) goes once the ask is answered: it doesn't stay above the composer.
#[gpui::test]
fn answering_an_ask_clears_the_error_left_about_it(cx: &mut TestAppContext) {
    let (one, cx) = one_view(cx, Provider::Claude, fake(), None);
    send(&one, "permit the deploy", cx);
    wait_until(cx, "the ask", |cx| !approvals(&one, cx).is_empty());
    let id = approvals(&one, cx)[0].0.clone();
    one.view.update(cx, |v, cx| {
        v.show_error(anyhow::anyhow!("No project rule can cover this call"), cx);
        v.answer(&id, Decision::AllowOnce, cx);
    });
    wait_until(cx, "the allow to reach the agent", |cx| answers(&one, cx).len() == 1);
    assert_eq!(one.view.read_with(cx, |v, _| v.error.clone()), None);
}
