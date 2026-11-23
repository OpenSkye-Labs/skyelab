//! A remote follow-up or interrupt is answered once it reached the agent, or with why it didn't, never as soon as
//! the app queued it.

use super::calls::{Fixture, connect_hello, error_of, sharing_workspace};
use super::*;

/// Starts a thread from the client and waits for the fake agent to report itself; returns (thread id, its pid).
fn started(f: &Fixture, client: &Client, cx: &mut VisualTestContext) -> (i64, u32) {
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let mut events = Vec::new();
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: "hello".into(),
        },
    );
    let thread_id = match client.reply(2, &mut events, cx) {
        ServerMessage::Response {
            result: Answer::ThreadStarted { thread_id },
            ..
        } => thread_id,
        other => panic!("start failed: {other:?}"),
    };
    let mut pid = None;
    wait_until(cx, "the fake agent's pid", |_| {
        while let Ok(msg) = client.inbox.try_recv() {
            if let ServerMessage::Event(e) = msg {
                events.push(e);
            }
        }
        // The fake's `fake_env` line ends with `"pid":<n>}`, whatever the row's envelope around it.
        pid = events.iter().find_map(|e| {
            if !e.payload_json.contains("fake_env") {
                return None;
            }
            let (_, rest) = e.payload_json.split_once("\"pid\":")?;
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        });
        pid.is_some()
    });
    let pid = pid.expect("waited for it");
    assert!(pid > 1, "a real child's pid, never a process group: {pid}");
    (thread_id, pid)
}

/// Resumes the stopped fake however the test ends, so a failed assert never leaves it stopped.
#[cfg(unix)]
struct Resume(u32);

#[cfg(unix)]
impl Drop for Resume {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/bin/kill")
            .args(["-CONT", &self.0.to_string()])
            .status();
    }
}

fn send_message(client: &Client, id: u64, thread_id: i64, text: String) {
    client.send(id, Call::SendMessage { thread_id, text });
}

#[gpui::test]
fn remote_send_replies_after_delivery(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let (thread_id, _) = started(&f, &client, cx);
    // The session still runs, but the row says the agent ended (a Stop or an exit got there first): the follow-up's
    // delivery step refuses it, and that refusal is the answer, not the "queued" Ok it used to get.
    f.store
        .lock()
        .expect("lock")
        .set_thread_status(thread_id, "stopped")
        .expect("status");
    send_message(&client, 3, thread_id, "too late".into());
    let error = error_of(client.reply(3, &mut Vec::new(), cx));
    assert!(error.message.contains("has ended"), "{error:?}");
}

#[cfg(unix)]
#[gpui::test]
fn remote_interrupt_replies_after_delivery(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let (thread_id, pid) = started(&f, &client, cx);
    client.send(3, Call::Interrupt { thread_id });
    assert!(
        matches!(
            client.reply(3, &mut Vec::new(), cx),
            ServerMessage::Response {
                result: Answer::Ok,
                ..
            }
        ),
        "a running agent takes the interrupt"
    );

    // An agent that stops reading: one big follow-up fills its pipe, then interrupts fill the input queue. Once
    // it is full, an interrupt can't be delivered, and the client hears that.
    let stopped = std::process::Command::new("/bin/kill")
        .args(["-STOP", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(stopped.success());
    let _resume = Resume(pid);
    send_message(&client, 4, thread_id, "x".repeat(256 << 10));
    assert!(matches!(
        client.reply(4, &mut Vec::new(), cx),
        ServerMessage::Response { .. }
    ));
    let mut refused = None;
    for id in 5..200 {
        client.send(id, Call::Interrupt { thread_id });
        if let ServerMessage::Error { error, .. } = client.reply(id, &mut Vec::new(), cx) {
            refused = Some(error);
            break;
        }
    }
    let error = refused.expect("an interrupt the agent can't take is refused");
    assert!(error.message.contains("isn't reading"), "{error:?}");
}
