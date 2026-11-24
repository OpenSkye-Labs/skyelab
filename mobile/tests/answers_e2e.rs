//! Acceptance: from the phone, a waiting AskUserQuestion is answered with the picked labels, and a proposed plan is
//! approved or sent back to planning, through the protocol's own calls to a real (in-process) daemon.

pub mod common;

use std::time::{Duration, Instant};

use common::{NODE_NAME, Ran, options, start};
use gpui::TestAppContext;
use serde_json::json;
use skyelab_agent::Event;
use skyelab_mobile::{app::MobileApp, remote::RemoteSource, source::ThreadSource};
use skyelab_protocol::PermissionAnswer;

fn ask(id: &str, tool: &str, input: serde_json::Value) -> String {
    serde_json::to_string(&Event::Permission {
        id: id.into(),
        tool: tool.into(),
        input,
    })
    .expect("serializes")
}

#[gpui::test]
fn questions_and_plans_are_answered_on_the_desktop(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = start();
    let t = env.thread_id;
    let questions = json!({"questions": [{
        "header": "Theme", "question": "Which theme?", "multiSelect": false,
        "options": [{"label": "A", "description": "first"}, {"label": "B", "description": "second"}]
    }]});
    env.append(t, "agent", &ask("q1", "AskUserQuestion", questions));
    env.append(
        t,
        "agent",
        &ask("plan1", "ExitPlanMode", json!({"plan": "1. Do it"})),
    );
    env.append(
        t,
        "agent",
        &ask("plan2", "ExitPlanMode", json!({"plan": "2. Redo it"})),
    );
    let port = env.port;
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let (app, cx) = cx.add_window_view(|window, cx| {
        let pins = data.clone();
        let connect = Box::new(move |_: &str| {
            Ok(
                Box::new(RemoteSource::connect(NODE_NAME, options(port), &pins)?)
                    as Box<dyn ThreadSource>,
            )
        });
        MobileApp::new(data.clone(), connect, window, cx)
    });
    let input = app.read_with(cx, |a, _| a.env_input.clone());
    input.update_in(cx, |s, window, cx| {
        s.set_value("desktop.example.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));

    let wait = |cx: &mut gpui::VisualTestContext, what: &str, done: &dyn Fn(&MobileApp) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            cx.run_until_parked();
            if app.update(cx, |a, _| {
                a.sync_items();
                done(a)
            }) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    wait(cx, "threads", &|a| !a.threads().is_empty());
    app.update_in(cx, |a, window, cx| a.open_thread(t, window, cx));
    wait(cx, "the asks", &|a| a.items().len() >= 5);
    app.read_with(cx, |a, _| {
        assert!(a.can_answer_questions(), "a 1.4 machine")
    });

    app.update(cx, |a, cx| a.pick("q1", 0, 1, cx));
    app.update(cx, |a, cx| a.submit_answers("q1", cx));
    app.update(cx, |a, cx| {
        a.answer("plan1", PermissionAnswer::AllowOnce, cx)
    });
    app.update(cx, |a, cx| a.answer("plan2", PermissionAnswer::Deny, cx));

    let deadline = Instant::now() + Duration::from_secs(5);
    let want = [
        Ran::Questions(t, "q1".into(), vec![vec!["B".into()]]),
        Ran::Answer(t, "plan1".into(), PermissionAnswer::AllowOnce),
        Ran::Answer(t, "plan2".into(), PermissionAnswer::Deny),
    ];
    while !want.iter().all(|w| env.ran().contains(w)) {
        assert!(Instant::now() < deadline, "ran: {:?}", env.ran());
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_question_needs_a_pick_before_submit() {
    use skyelab_mobile::app::ask_rows::answers;
    let qs = skyelab_agent::timeline::cards::questions(&json!({"questions": [
        {"question": "One?", "options": [{"label": "x"}, {"label": "y"}]},
        {"question": "Many?", "multiSelect": true, "options": [{"label": "p"}, {"label": "q"}]}
    ]}));
    let none = Vec::new();
    assert_eq!(
        answers(&qs, &none, &[]),
        Err("Pick an answer to \"One?\".".into())
    );
    let picked = vec![[1].into_iter().collect(), [0, 1].into_iter().collect()];
    assert_eq!(
        answers(&qs, &picked, &[]),
        Ok(vec![vec!["y".into()], vec!["p".into(), "q".into()]])
    );
}
