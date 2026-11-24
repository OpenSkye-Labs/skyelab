//! AskUserQuestion end to end on the fake claude: it asks, the user picks in the card (a single-choice option, a
//! multi-choice option plus typed "Other" text) and submits, and the agent gets Claude's answer shape. ExitPlanMode:
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
fn answers_picked_in_the_card_reach_the_agent(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question card", |cx| ask(&one, cx).is_some());
    // The card is the last row: scroll it to the top so every option is on screen.
    one.view.update(cx, |v, _| {
        let item_ix = v.items.len() - 1;
        v.own_list().scroll_to(gpui::ListOffset {
            item_ix,
            offset_in_item: gpui::px(0.),
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("question-card").is_some());
    // Question 1 is single-choice: the second click replaces the first.
    click(cx, "question-option-0-0");
    click(cx, "question-option-0-1");
    // Question 2 is multi-choice: Cache, and Other with typed text.
    click(cx, "question-option-1-0");
    let other = format!("question-option-1-{}", usize::MAX);
    let other: &'static str = Box::leak(other.into_boxed_str());
    click(cx, other);
    let (id, _) = ask(&one, cx).expect("asked");
    // Submitting before the Other text is typed is refused, and says why.
    one.view.update(cx, |v, cx| v.submit_answers(&id, cx));
    assert!(
        one.view
            .read_with(cx, |v, _| v.error.clone())
            .is_some_and(|e| e.contains("Type your answer")),
        "an empty Other answer is refused: {:?} {:?}",
        one.view.read_with(cx, |v, _| v.error.clone()),
        one.view
            .read_with(cx, |v, _| v.drafts.get(&id).map(|d| d.picked.clone()))
    );
    let input = one.view.read_with(cx, |v, _| {
        v.drafts[&id].other[1]
            .clone()
            .expect("the Other field exists")
    });
    input.update_in(cx, |i, window, cx| i.set_value("Redis", window, cx));
    one.view.update(cx, |v, cx| v.submit_answers(&id, cx));
    wait_until(cx, "the agent's report of the answer", |cx| {
        reply(&one, cx).is_some()
    });
    let r = reply(&one, cx).expect("reply");
    assert_eq!(r["request_id"], "q-1");
    assert_eq!(r["response"]["behavior"], "allow");
    assert_eq!(
        r["response"]["updatedInput"]["answers"],
        json!({ "Which database?": "SQLite", "Which extras?": "Cache, Redis" })
    );
    wait_until(cx, "the card to say it was answered", |cx| {
        ask(&one, cx).is_some_and(|(_, d)| d == Some(Decision::AllowOnce))
    });
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
            "Which database?": "SQLite, Attached files: /tmp/schema.sql",
            "Which extras?": "Attached files: /tmp/notes.md",
        })
    );
}

#[test]
fn an_answer_takes_at_most_a_hundred_files() {
    use super::question_drafts::{Drafts, attach};
    let mut drafts = Drafts::new();
    let many: Vec<std::path::PathBuf> = (0..150).map(|i| format!("/f{i}").into()).collect();
    // No item: the draft still sizes to the question index given by an earlier pick, so size it by hand.
    drafts.entry("q".into()).or_default().files = vec![Vec::new()];
    attach(&[], &mut drafts, "q", 0, many);
    assert_eq!(drafts["q"].files[0].len(), 100);
}
