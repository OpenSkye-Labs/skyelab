//! A permission request on this machine is answered from another device: the client sees the pending request in
//! the thread's pushed events, answers it with `answer_permission`, and the answer reaches the agent (the fake
//! reports the control_response it got).

use super::calls::{connect_hello, sharing_workspace};
use super::*;

fn agent_payloads(events: &[skyelab_protocol::ThreadEvent]) -> Vec<serde_json::Value> {
    events
        .iter()
        .filter(|e| e.kind == "agent")
        .filter_map(|e| serde_json::from_str(&e.payload_json).ok())
        .collect()
}

#[gpui::test]
fn a_remote_client_answers_a_pending_permission(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let mut events = Vec::new();
    // The user picked Ask as the new-thread default; with Full access (the default) nothing would ask.
    cx.update(|_, cx| {
        crate::workspace::thread_extras::prefs::edit(
            |t| t.permission_mode = Some("default".into()),
            cx,
        )
    })
    .detach();
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: "permit the deploy".into(),
        },
    );
    let thread_id = match client.reply(2, &mut events, cx) {
        ServerMessage::Response {
            result: Answer::ThreadStarted { thread_id },
            ..
        } => thread_id,
        other => panic!("start failed: {other:?}"),
    };
    let pump = |events: &mut Vec<skyelab_protocol::ThreadEvent>| {
        while let Ok(msg) = client.inbox.try_recv() {
            if let ServerMessage::Event(e) = msg {
                events.push(e);
            }
        }
    };
    let mut asked = String::new();
    wait_until(cx, "the pending request pushed", |_| {
        pump(&mut events);
        if let Some(id) = agent_payloads(&events)
            .iter()
            .filter(|v| v["Permission"]["tool"] == "Bash")
            .find_map(|v| v["Permission"]["id"].as_str().map(str::to_owned))
        {
            asked = id;
        }
        !asked.is_empty()
    });

    // A request id the agent isn't waiting on is refused, and nothing reaches it.
    client.send(
        3,
        Call::AnswerPermission {
            thread_id,
            request_id: "perm-9".into(),
            decision: skyelab_protocol::PermissionAnswer::AllowOnce,
        },
    );
    assert!(matches!(
        client.reply(3, &mut events, cx),
        ServerMessage::Error { .. }
    ));
    client.send(
        4,
        Call::AnswerPermission {
            thread_id,
            request_id: asked.clone(),
            decision: skyelab_protocol::PermissionAnswer::Deny,
        },
    );
    assert!(matches!(
        client.reply(4, &mut events, cx),
        ServerMessage::Response {
            result: Answer::Ok,
            ..
        }
    ));
    wait_until(cx, "the agent's report of the answer", |_| {
        pump(&mut events);
        agent_payloads(&events).iter().any(|v| {
            v["Unknown"]["type"] == "fake_answer"
                && v["Unknown"]["reply"]["response"]["response"]["behavior"] == "deny"
        })
    });
    assert!(
        agent_payloads(&events)
            .iter()
            .any(|v| v["PermissionResolved"]["decision"] == "deny"),
        "the decision is stored and pushed, so every viewer sees it answered"
    );
}
