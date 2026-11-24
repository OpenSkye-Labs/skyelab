//! Acceptance: a waiting AskUserQuestion whose question has no options is answered from the phone with typed text,
//! through the protocol's own call to a real (in-process) daemon; empty text is refused before anything is sent.

pub mod common;

use std::time::{Duration, Instant};

use common::{NODE_NAME, Ran, options, start};
use gpui::TestAppContext;
use serde_json::json;
use skyelab_agent::Event;
use skyelab_mobile::{app::MobileApp, remote::RemoteSource, source::ThreadSource};

#[gpui::test]
fn a_question_without_options_takes_typed_text(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_component::init);
    let env = start();
    let t = env.thread_id;
    let ask = Event::Permission {
        id: "q1".into(),
        tool: "AskUserQuestion".into(),
        input: json!({"questions": [{"header": "Name", "question": "What should it be called?", "options": []}]}),
    };
    env.append(
        t,
        "agent",
        &serde_json::to_string(&ask).expect("serializes"),
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
    wait(cx, "the ask", &|a| {
        a.can_answer_questions() && !a.items().is_empty()
    });
    app.update_in(cx, |a, window, cx| a.ensure_typed(window, cx));
    let field = app
        .read_with(cx, |a, _| a.typed_answer("q1", 0))
        .expect("a text field for the question without options");

    // Nothing typed: refused, nothing sent.
    app.update(cx, |a, cx| a.submit_answers("q1", cx));
    let refused = app.read_with(cx, |a, _| a.error.clone());
    assert!(
        refused.is_some_and(|e| e.contains("Type an answer")),
        "empty text was not refused"
    );

    field.update_in(cx, |s, window, cx| s.set_value("  Skye  ", window, cx));
    app.update(cx, |a, cx| a.submit_answers("q1", cx));
    let want = Ran::Questions(t, "q1".into(), vec![vec!["Skye".into()]]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !env.ran().contains(&want) {
        assert!(Instant::now() < deadline, "ran: {:?}", env.ran());
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !env.ran()
            .iter()
            .any(|r| matches!(r, Ran::Questions(_, _, a) if a != &vec![vec!["Skye".to_string()]])),
        "only the typed answer was sent: {:?}",
        env.ran()
    );
}
