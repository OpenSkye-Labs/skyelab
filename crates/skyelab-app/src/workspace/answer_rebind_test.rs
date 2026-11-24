//! Approving a waiting plan after the folder was unbound must not reach the old account's agent: it ends and the
//! thread shows "This thread can't continue here", like any other send to a live agent.

use super::claude_folded_turns_test::{one_view, wait_until};
use super::thread_model::{Item, Record};
use gpui::TestAppContext;
use skyelab_agent::{Event, Mode};
use skyelab_core::Provider;
use std::path::Path;

#[gpui::test]
fn approving_a_plan_after_an_unbind_never_reaches_the_agent(cx: &mut TestAppContext) {
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    let (one, cx) = one_view(cx, Provider::Claude, bin, None);
    one.view.update(cx, |v, _| v.mode = Mode::Plan);
    one.view
        .update_in(cx, |v, window, cx| v.submit_remote("plan it", window, cx))
        .expect("started");
    let ask = |cx: &mut gpui::VisualTestContext| {
        one.view.read_with(cx, |v, _| {
            v.items.iter().find_map(|i| match i {
                Item::Approval(a) => Some(a.id.clone()),
                _ => None,
            })
        })
    };
    wait_until(cx, "the plan card", |cx| ask(cx).is_some());
    let id = ask(cx).expect("asked");
    let mut config = one.view.read_with(cx, |v, _| v.config.clone());
    config.bindings.clear();
    one.view.update(cx, |v, cx| v.set_config(config, cx));
    one.view.update(cx, |v, cx| v.answer_plan(&id, true, cx));
    let got_answer = |cx: &mut gpui::VisualTestContext| {
        let Some(tid) = one
            .view
            .read_with(cx, |v, _| v.thread.as_ref().map(|t| t.id))
        else {
            return false;
        };
        let s = one.store.lock().expect("lock");
        s.events_since(tid, 0).expect("events").iter().any(|e| {
            matches!(Record::from_row(&e.kind, &e.payload_json),
                Ok(Record::Agent(Event::Unknown(v))) if v["type"] == "fake_answer")
        })
    };
    wait_until(cx, "the answer or the refusal", |cx| {
        got_answer(cx) || one.view.read_with(cx, |v, _| v.session.is_none())
    });
    assert!(
        !got_answer(cx),
        "plan approval reached the agent after the folder was unbound"
    );
    assert!(
        one.view
            .read_with(cx, |v, _| v.session.is_none() && v.resume_refused.is_some()),
        "the agent is ended and the can't-continue notice shows"
    );
}
