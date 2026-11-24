//! AskUserQuestion end to end on the fake claude: it asks, the user answers in the panel docked on the composer (a
//! click, a digit, typed text) and submits, and the agent gets Claude's answer shape. ExitPlanMode:
//! Approve allows and leaves the thread in Ask mode; Keep planning denies.

use super::claude_folded_turns_test::{One, one_view, wait_until};
use super::thread_model::{Item, Record};
use gpui::{TestAppContext, VisualTestContext};
use serde_json::{Value, json};
use skyelab_agent::{Decision, Event, Mode};
use skyelab_core::Provider;
use std::path::Path;

fn start<'a>(
    prompt: &str,
    mode: Mode,
    cx: &'a mut TestAppContext,
) -> (One, &'a mut VisualTestContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update(cx, |v, _| v.mode = mode);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote(prompt, window, cx))
        .expect("started");
    (one, cx)
}

fn ask(one: &One, cx: &mut VisualTestContext) -> Option<(String, Option<Decision>)> {
    one.view.read_with(cx, |v, _| {
        v.items.iter().find_map(|i| match i {
            Item::Approval(a) => Some((a.id.clone(), a.decision)),
            _ => None,
        })
    })
}

/// The reply the fake reported getting.
fn reply(one: &One, cx: &mut VisualTestContext) -> Option<Value> {
    let id = one
        .view
        .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))?;
    let s = one.store.lock().expect("lock");
    s.events_since(id, 0).expect("events").iter().find_map(|e| {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(Event::Unknown(v))) if v["type"] == "fake_answer" => {
                Some(v["reply"]["response"].clone())
            }
            _ => None,
        }
    })
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let at = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(at.center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn answers_given_in_the_docked_panel_reach_the_agent(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    // The question is docked on the composer (T3's), one at a time; the timeline keeps one line.
    assert!(cx.debug_bounds("question-dock").is_some(), "the panel is drawn");
    assert!(cx.debug_bounds("waiting-row").is_some(), "the timeline row says to answer below");
    assert!(cx.debug_bounds("question-card").is_none(), "no second copy in the timeline");
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((0, 2)));
    // Question 1 is single-choice: a click picks and moves on.
    click(cx, "dock-option-0");
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)));
    // Back to question 1 by hand, then a lone digit in the empty composer picks option 2 and moves on.
    let id = ask(&one, cx).expect("asked").0;
    one.view.update(cx, |v, _| v.drafts.get_mut(&id).expect("draft").step = 0);
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("2", window, cx));
        v.dock_digit(window, cx);
    });
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)));
    assert_eq!(
        one.view.read_with(cx, |v, cx| v.prompt.read(cx).value().to_string()),
        "",
        "the digit picked, it wasn't typed"
    );
    // Question 2 is multi-choice: Submit with nothing picked or typed is refused, and says why.
    one.view.update_in(cx, |v, window, cx| v.send_prompt(&super::thread::SendPrompt, window, cx));
    assert!(
        one.view
            .read_with(cx, |v, _| v.composer.notice.clone())
            .is_some_and(|n| n.contains("Pick an option")),
        "an empty answer is refused"
    );
    // A pick, then text in the composer: the text is the answer, as T3's "leave this blank to use the selected option".
    click(cx, "dock-option-0");
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)), "a multi pick stays");
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("Redis", window, cx));
        v.send_prompt(&super::thread::SendPrompt, window, cx);
    });
    wait_until(cx, "the agent's report of the answer", |cx| {
        reply(&one, cx).is_some()
    });
    let r = reply(&one, cx).expect("reply");
    assert_eq!(r["request_id"], "q-1");
    assert_eq!(r["response"]["behavior"], "allow");
    assert_eq!(
        r["response"]["updatedInput"]["answers"],
        json!({ "Which database?": "SQLite", "Which extras?": "Redis" })
    );
    wait_until(cx, "the question to say it was answered", |cx| {
        ask(&one, cx).is_some_and(|(_, d)| d == Some(Decision::AllowOnce))
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("question-dock").is_none(), "the panel goes once answered");
}

#[gpui::test]
fn approving_a_plan_allows_it_and_leaves_plan_mode(cx: &mut TestAppContext) {
    let (one, cx) = start("plan it", Mode::Plan, cx);
    wait_until(cx, "the plan card", |cx| ask(&one, cx).is_some());
    let (id, _) = ask(&one, cx).expect("asked");
    one.view.update(cx, |v, cx| v.answer_plan(&id, true, cx));
    wait_until(cx, "the agent's report", |cx| reply(&one, cx).is_some());
    assert_eq!(
        reply(&one, cx).expect("reply")["response"]["behavior"],
        "allow"
    );
    assert_eq!(
        one.view.read_with(cx, |v, _| v.mode),
        Mode::Default,
        "Ask mode after Approve"
    );
}

#[gpui::test]
fn keep_planning_denies_the_plan(cx: &mut TestAppContext) {
    let (one, cx) = start("plan it", Mode::Plan, cx);
    wait_until(cx, "the plan card", |cx| ask(&one, cx).is_some());
    let (id, _) = ask(&one, cx).expect("asked");
    one.view.update(cx, |v, cx| v.answer_plan(&id, false, cx));
    wait_until(cx, "the agent's report", |cx| reply(&one, cx).is_some());
    assert_eq!(
        reply(&one, cx).expect("reply")["response"]["behavior"],
        "deny"
    );
}

#[gpui::test]
fn files_attached_to_answers_reach_the_agent_as_paths(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question card", |cx| ask(&one, cx).is_some());
    let (id, _) = ask(&one, cx).expect("asked");
    let (a, b) = (Path::new("/tmp/schema.sql"), Path::new("/tmp/notes.md"));
    one.view.update(cx, |v, cx| {
        v.pick(&id, 0, 1, false, cx);
        v.attach_answer_paths(&id, 0, vec![a.into()], cx);
        // Question 2 is answered by files alone; the same file twice is kept once.
        v.attach_answer_paths(&id, 1, vec![b.into(), b.into()], cx);
    });
    one.view.update(cx, |v, cx| v.submit_answers(&id, cx));
    assert_eq!(
        one.view.read_with(cx, |v, _| v.error.clone()),
        None,
        "files alone answer a question"
    );
    wait_until(cx, "the agent's report of the answer", |cx| {
        reply(&one, cx).is_some()
    });
    let r = reply(&one, cx).expect("reply");
    assert_eq!(
        r["response"]["updatedInput"]["answers"],
        json!({
            "Which database?": "SQLite, Attached files: \"/tmp/schema.sql\"",
            "Which extras?": "Attached files: \"/tmp/notes.md\"",
        })
    );
}

#[test]
fn an_answer_takes_at_most_a_hundred_files() {
    use super::question_drafts::{Drafts, attach};
    let mut drafts = Drafts::new();
    let many: Vec<std::path::PathBuf> = (0..150).map(|i| format!("/f{i}").into()).collect();
    // With no question item to size it, the draft gets its one question by hand.
    drafts.entry("q".into()).or_default().files = vec![Vec::new()];
    attach(&[], &mut drafts, "q", 0, many);
    assert_eq!(drafts["q"].files[0].len(), 100);
}
