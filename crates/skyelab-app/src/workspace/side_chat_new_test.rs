//! A side chat opened empty (the details card, `s`, New side chat) takes the focus, so what you type next goes to it,
//! and it doesn't say its agent ended: nothing has run there yet.

use super::*;
use crate::workspace::worktree_thread_test::{fixture, open_thread, send, wait_until};

#[gpui::test]
fn an_empty_side_chat_takes_the_focus_and_claims_no_ended_agent(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() && !t.turn_running())
    });
    thread.update(cx, |_, cx| {
        cx.emit(ThreadViewEvent::Side(SideAsk::New(None)))
    });
    wait_until(cx, "the side chat pane", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    cx.run_until_parked();
    let focused = side.update_in(cx, |v, window, cx| {
        v.prompt.read(cx).focus_handle(cx).is_focused(window)
    });
    assert!(focused, "typing goes to the new side chat");
    assert_eq!(side.read_with(cx, |v, _| v.agent_note(false)), None);
}
