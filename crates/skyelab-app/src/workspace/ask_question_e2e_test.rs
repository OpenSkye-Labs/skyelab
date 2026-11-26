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
    // A click on Next that was on its way when the pick moved on lands on "Submit answers": ignored, not an empty
    // submit with "Pick an option".
    click(cx, "dock-next");
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)));
    assert!(one.view.read_with(cx, |v, _| v.composer.notice.is_none()), "the quick click did nothing");
    one.view.update(cx, |v, _| v.composer.dock_advanced = None); // the guard's half second has passed
    // Back to question 1 by hand, then 2 on the panel picks option 2 and moves on; 9 has no option.
    let id = ask(&one, cx).expect("asked").0;
    one.view.update(cx, |v, _| v.drafts.get_mut(&id).expect("draft").step = 0);
    let used = one.view.update_in(cx, |v, window, cx| v.dock_key("9", window, cx));
    assert!(!used, "no option 9");
    let used = one.view.update_in(cx, |v, window, cx| v.dock_key("2", window, cx));
    assert!(used);
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)));
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
fn a_typed_answer_keeps_its_files_and_a_second_submit_keeps_it(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    let id = ask(&one, cx).expect("asked").0;
    one.view.update(cx, |v, cx| {
        v.pick(&id, 0, 1, false, cx);
        v.attach_answer_paths(&id, 1, vec![Path::new("/tmp/schema.sql").into()], cx);
        v.drafts.get_mut(&id).expect("draft").step = 1;
    });
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("Redis", window, cx));
        v.send_prompt(&super::thread::SendPrompt, window, cx);
        // A quick second Submit while the first is on its way changes nothing.
        v.send_prompt(&super::thread::SendPrompt, window, cx);
        assert_eq!(v.drafts[&id].typed[1], "Redis");
    });
    wait_until(cx, "the agent's report of the answer", |cx| reply(&one, cx).is_some());
    let r = reply(&one, cx).expect("reply");
    assert_eq!(
        r["response"]["updatedInput"]["answers"]["Which extras?"],
        "Redis, Attached files: \"/tmp/schema.sql\""
    );
    wait_until(cx, "the composer to empty once answered", |cx| {
        one.view.read_with(cx, |v, cx| v.prompt.read(cx).value().is_empty())
    });
}

#[gpui::test]
fn side_and_steer_are_not_taken_as_an_answer(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("/side what is SQLite?", window, cx));
        v.send_prompt(&super::thread::SendPrompt, window, cx);
    });
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((0, 2)));
    let typed = one.view.read_with(cx, |v, _| {
        v.drafts.values().any(|d| d.typed.iter().any(|t| !t.is_empty()))
    });
    assert!(!typed, "/side didn't become the question's answer");
}

#[gpui::test]
fn a_message_declines_a_docked_permission(cx: &mut TestAppContext) {
    let (one, cx) = start("permit", Mode::Default, cx);
    wait_until(cx, "the ask", |cx| ask(&one, cx).is_some());
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("don't deploy, run the tests", window, cx));
        v.send_prompt(&super::thread::SendPrompt, window, cx);
    });
    wait_until(cx, "the ask to be declined", |cx| {
        ask(&one, cx).is_some_and(|(_, d)| d == Some(Decision::Deny))
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

/// Whether the docked question's panel has the focus.
fn panel_focused(one: &One, cx: &mut VisualTestContext) -> bool {
    one.view.update_in(cx, |v, window, _| {
        v.composer.dock_focus.as_ref().is_some_and(|f| f.is_focused(window))
    })
}

fn prompt_focused(one: &One, cx: &mut VisualTestContext) -> bool {
    one.view.update_in(cx, |v, window, cx| {
        gpui::Focusable::focus_handle(v.prompt.read(cx), cx).is_focused(window)
    })
}

/// A question that docks takes the focus, so the keys answer it at once (T3's 1–9 work without a click); Esc goes
/// back to the composer, and FocusQuestion comes back.
#[gpui::test]
fn a_docked_question_takes_the_keys_without_a_click(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    one.view.update_in(cx, |v, window, cx| {
        window.focus(&gpui::Focusable::focus_handle(v.prompt.read(cx), cx), cx)
    });
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    assert!(panel_focused(&one, cx), "the panel took the focus from the empty composer");
    // ↓ ↓ picks the second option and stays on question 1; Enter moves on.
    cx.simulate_keystrokes("down down");
    let id = ask(&one, cx).expect("asked").0;
    let picked = |cx: &mut VisualTestContext| {
        one.view
            .read_with(cx, |v, _| v.drafts[&id].picked[0].iter().copied().collect::<Vec<_>>())
    };
    assert_eq!(picked(cx), [1]);
    cx.simulate_keystrokes("up");
    assert_eq!(picked(cx), [0]);
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((0, 2)));
    cx.simulate_keystrokes("enter");
    assert_eq!(one.view.read_with(cx, |v, _| v.dock_step()), Some((1, 2)));
    cx.simulate_keystrokes("escape");
    assert!(prompt_focused(&one, cx), "Esc is back in the composer");
    cx.dispatch_action(super::ask_dock::FocusQuestion);
    assert!(panel_focused(&one, cx), "FocusQuestion goes back to the panel");
    // A digit is an option here, not text.
    cx.simulate_keystrokes("1");
    assert_eq!(
        one.view.read_with(cx, |v, _| v.drafts[&id].picked[1].iter().copied().collect::<Vec<_>>()),
        [0]
    );
    assert!(one.view.read_with(cx, |v, cx| v.prompt.read(cx).value().is_empty()));
}

/// Text being typed in the composer keeps the focus when a question docks (T3 leaves keys typed in a field alone).
#[gpui::test]
fn a_docked_question_leaves_a_composer_being_typed_in(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    one.view.update_in(cx, |v, window, cx| {
        v.prompt.update(cx, |p, cx| p.set_value("half a thought", window, cx));
        window.focus(&gpui::Focusable::focus_handle(v.prompt.read(cx), cx), cx)
    });
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    assert!(prompt_focused(&one, cx), "the composer kept the focus");
    assert!(!panel_focused(&one, cx));
}

/// Answered from the keys alone, the question goes and its keys go back to the composer, not to a panel no longer
/// drawn; a letter typed on the panel starts the typed answer in the composer.
#[gpui::test]
fn the_keys_come_back_to_the_composer(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    assert!(panel_focused(&one, cx));
    cx.simulate_keystrokes("2 h i");
    assert!(prompt_focused(&one, cx), "a letter moves to the composer");
    assert_eq!(one.view.read_with(cx, |v, cx| v.prompt.read(cx).value().to_string()), "hi");
    cx.dispatch_action(super::ask_dock::FocusQuestion);
    cx.simulate_keystrokes("enter");
    wait_until(cx, "the answer taken", |cx| {
        ask(&one, cx).is_some_and(|(_, d)| d.is_some())
    });
    cx.run_until_parked();
    assert!(prompt_focused(&one, cx), "the composer has the keys again");
}

/// With nothing focused, a question in a thread the window doesn't have selected (a side chat, a Compare pane beside
/// it) leaves the focus alone: several shown threads can get one at once, and only the selected one takes the keys.
#[gpui::test]
fn only_the_selected_thread_takes_the_keys_for_its_question(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    one.view.update_in(cx, |_, window, cx| {
        let elsewhere = gpui::AppContext::new(cx, |_| ()).entity_id(); // the page is another thread
        cx.default_global::<super::center::ShownThread>()
            .0
            .insert(window.window_handle().window_id(), Some(elsewhere));
    });
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    assert!(!panel_focused(&one, cx), "not the selected thread");
}

/// A page with no selected thread (Compare, Welcome): with nothing focused, the pane showing the question (a side
/// chat in the dock) takes the keys. Only the first one does: once it has the focus, something is focused.
#[gpui::test]
fn without_a_selected_thread_the_shown_pane_takes_the_keys(cx: &mut TestAppContext) {
    let (one, cx) = start("question please", Mode::Default, cx);
    one.view.update_in(cx, |_, window, cx| {
        cx.default_global::<super::center::ShownThread>()
            .0
            .insert(window.window_handle().window_id(), None); // a Compare page
        window.blur(cx);
    });
    wait_until(cx, "the question", |cx| ask(&one, cx).is_some());
    cx.run_until_parked();
    assert!(panel_focused(&one, cx), "the only shown pane took the keys");
}

#[test]
fn the_button_says_what_it_does_next() {
    use super::ask_dock::next_label;
    assert_eq!(next_label(0, 2), "Next question");
    assert_eq!(next_label(1, 2), "Submit answers");
    assert_eq!(next_label(0, 1), "Submit answer");
}
