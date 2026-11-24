//! An AskUserQuestion and an ExitPlanMode plan on this machine are answered from another device: the questions with
//! `answer_questions` (1.4), the plan with `answer_permission`, whose approval also takes the thread out of Plan as a
//! click on the card does. A revoked device's queued answer never reaches the agent.

use super::calls::{connect_hello, sharing_workspace};
use super::*;
use skyelab_agent::Mode;
use skyelab_protocol::{PermissionAnswer, ThreadEvent, device_key_hash};

fn agent_payloads(events: &[ThreadEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter(|e| e.kind == "agent")
        .filter_map(|e| serde_json::from_str(&e.payload_json).ok())
        .collect()
}

/// Starts a thread with `prompt` and waits for the fake's request for `tool`: (thread id, request id).
fn start_and_wait_for(
    client: &Client,
    prompt: &str,
    tool: &str,
    project_id: i64,
    events: &mut Vec<ThreadEvent>,
    cx: &mut VisualTestContext,
) -> (i64, String) {
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: prompt.into(),
        },
    );
    let thread_id = match client.reply(2, events, cx) {
        ServerMessage::Response {
            result: Answer::ThreadStarted { thread_id },
            ..
        } => thread_id,
        other => panic!("start failed: {other:?}"),
    };
    let mut asked = String::new();
    wait_until(cx, "the pending request pushed", |_| {
        while let Ok(msg) = client.inbox.try_recv() {
            if let ServerMessage::Event(e) = msg {
                events.push(e);
            }
        }
        if let Some(id) = agent_payloads(events)
            .iter()
            .filter(|v| v["Permission"]["tool"] == tool)
            .find_map(|v| v["Permission"]["id"].as_str().map(str::to_owned))
        {
            asked = id;
        }
        !asked.is_empty()
    });
    (thread_id, asked)
}

/// What the fake reported getting back, once it has.
fn fake_reply(f: &super::calls::Fixture, thread_id: i64) -> Option<serde_json::Value> {
    let store = f.store.lock().expect("store");
    let events = store.events_since(thread_id, 0).expect("events");
    events.iter().find_map(|e| {
        let v: serde_json::Value = serde_json::from_str(&e.payload_json).ok()?;
        (v["Unknown"]["type"] == "fake_answer").then(|| v["Unknown"]["reply"].clone())
    })
}

#[gpui::test]
fn a_remote_client_answers_a_question_and_approves_a_plan(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let mut events = Vec::new();

    let (thread_id, asked) = start_and_wait_for(
        &client,
        "question which db",
        "AskUserQuestion",
        project_id,
        &mut events,
        cx,
    );
    client.send(
        3,
        Call::AnswerQuestions {
            thread_id,
            request_id: asked,
            answers: vec![vec!["SQLite".into()], vec!["Cache".into(), "Queue".into()]],
        },
    );
    assert!(
        matches!(
            client.reply(3, &mut events, cx),
            ServerMessage::Response {
                result: Answer::Ok,
                ..
            }
        ),
        "answered"
    );
    let mut reply = None;
    wait_until(cx, "the agent's report of the answers", |_| {
        reply = fake_reply(&f, thread_id);
        reply.is_some()
    });
    let answers = &reply.expect("reply")["response"]["response"]["updatedInput"]["answers"];
    assert_eq!(answers["Which database?"], "SQLite");
    assert_eq!(answers["Which extras?"], "Cache, Queue");

    let (thread_id, asked) = start_and_wait_for(
        &client,
        "plan it",
        "ExitPlanMode",
        project_id,
        &mut events,
        cx,
    );
    f.ws.update(cx, |ws, cx| {
        ws.put_thread_in_mode(thread_id, Mode::Plan, cx)
    });
    client.send(
        4,
        Call::AnswerPermission {
            thread_id,
            request_id: asked,
            decision: PermissionAnswer::AllowOnce,
        },
    );
    assert!(matches!(
        client.reply(4, &mut events, cx),
        ServerMessage::Response {
            result: Answer::Ok,
            ..
        }
    ));
    wait_until(cx, "the thread leaves Plan", |cx| {
        f.ws.read_with(cx, |ws, cx| ws.thread_mode(thread_id, cx)) == Some(Mode::Default)
    });
    // The mode flips when the answer is sent; the fake reports it back later, so wait for that event too.
    let mut reply = None;
    wait_until(cx, "the agent's report of the approval", |_| {
        reply = fake_reply(&f, thread_id);
        reply.is_some()
    });
    let reply = reply.expect("the agent got the approval");
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
}

#[gpui::test]
fn a_revoked_device_cannot_answer_questions(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let mut events = Vec::new();
    let (thread_id, asked) = start_and_wait_for(
        &client,
        "question which db",
        "AskUserQuestion",
        project_id,
        &mut events,
        cx,
    );
    client.send(
        3,
        Call::AnswerQuestions {
            thread_id,
            request_id: asked,
            answers: vec![vec!["SQLite".into()], vec!["Cache".into()]],
        },
    );
    // Not pumping the app, so the call waits in the Workspace's queue.
    let deadline = std::time::Instant::now() + crate::test_wait::DEADLINE;
    while f.queue.is_empty() {
        assert!(std::time::Instant::now() < deadline, "never queued");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    f.remote.update(cx, |r, cx| {
        r.revoke("nPhone", &device_key_hash(PHONE_KEY), cx)
    });
    wait_until(cx, "the queue to drain", |_| f.queue.is_empty());
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        fake_reply(&f, thread_id),
        None,
        "the revoked device's answer reached the agent"
    );
}
