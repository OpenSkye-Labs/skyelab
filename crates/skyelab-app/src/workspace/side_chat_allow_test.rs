//! A side chat is read-only by construction: an Allow on its agent's permission request (a command it wants to run,
//! an edit) is refused, locally and from another device, and never reaches the agent; Deny still goes through.

use crate::workspace::thread::composer::Submit;
use crate::workspace::thread_model::{Item, Record};
use crate::workspace::worktree_thread_test::{Fixture, fixture, open_thread, send, wait_until};
use skyelab_agent::{Decision, Event};

fn reply(f: &Fixture, id: i64) -> Option<serde_json::Value> {
    let s = f.store.lock().expect("store");
    s.events_since(id, 0).expect("events").iter().find_map(|e| {
        match Record::from_row(&e.kind, &e.payload_json) {
            Ok(Record::Agent(Event::Unknown(v))) if v["type"] == "fake_answer" => {
                Some(v["reply"].clone())
            }
            _ => None,
        }
    })
}

#[gpui::test]
fn allowing_a_side_chats_command_never_reaches_its_agent(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() && !t.turn_running())
    });
    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("/side", window, cx));
        t.submit(Submit::Send, window, cx);
    });
    wait_until(cx, "the side chat pane", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side_view = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    wait_until(cx, "the side chat loads", |cx| {
        side_view.read_with(cx, |v, _| !v.read_only)
    });
    let side = side_view
        .read_with(cx, |v, _| v.thread.clone())
        .expect("row");
    // The first message carries the parent's context ahead of it, so the request comes in a follow-up.
    for prompt in ["hello", "permit the deploy"] {
        side_view
            .update_in(cx, |v, window, cx| v.submit_remote(prompt, window, cx))
            .expect("sent");
        if prompt == "hello" {
            wait_until(cx, "the first answer", |cx| {
                side_view.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
            });
        }
    }
    let ask = |cx: &mut gpui::VisualTestContext| {
        side_view.read_with(cx, |v, _| {
            v.items.iter().find_map(|i| match i {
                Item::Approval(a) => Some(a.id.clone()),
                _ => None,
            })
        })
    };
    wait_until(cx, "the request in the side chat", |cx| ask(cx).is_some());
    let id = ask(cx).expect("id");
    for decision in [Decision::AllowOnce, Decision::AllowThread] {
        let refused = side_view
            .update(cx, |v, cx| v.answer_then(&id, decision, |_| {}, cx))
            .expect_err("an Allow is refused");
        assert!(refused.contains("side chat"), "{refused}");
    }
    side_view
        .update(cx, |v, cx| v.answer_then(&id, Decision::Deny, |_| {}, cx))
        .expect("Deny goes through");
    wait_until(cx, "the agent's report of the answer", |_| {
        reply(&f, side.id).is_some()
    });
    let r = reply(&f, side.id).expect("reply");
    assert_eq!(r["response"]["response"]["behavior"], "deny", "{r}");

    // A file name on one of its cards opens the Preview tab too.
    side_view.update(cx, |v, cx| v.open_path("README.md", cx));
    cx.run_until_parked();
    let tab = ws
        .read_with(cx, |ws, _| ws.preview_tab())
        .expect("the Preview tab");
    assert!(tab.read_with(cx, |p, _| p.path.ends_with("README.md")));
}
