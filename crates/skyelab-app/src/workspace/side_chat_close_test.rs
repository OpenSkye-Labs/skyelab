//! `/side` before the thread has a row keeps the question; a side chat can't start side chats of its own; closing
//! the parent thread stops its side chats and takes their panes out.

use super::*;
use crate::workspace::thread::composer::Submit;
use crate::workspace::worktree_thread_test::{fixture, open_thread, send, wait_until};

fn ask(thread: &Entity<ThreadView>, text: &str, cx: &mut gpui::VisualTestContext) {
    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value(text, window, cx));
        t.submit(Submit::Send, window, cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn side_chats_refuse_early_stay_flat_and_close_with_their_parent(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    ask(&thread, "/side too early", cx);
    thread.read_with(cx, |t, cx| {
        assert_eq!(t.prompt.read(cx).value().as_ref(), "/side too early");
        let notice = t.composer.notice.clone().expect("why");
        assert!(notice.contains("Send a message first"), "{notice}");
    });
    assert!(ws.read_with(cx, |ws, _| ws.side_chats.is_empty()));

    thread.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value("", window, cx))
    });
    send(cx);
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| {
            t.thread.is_some() && !t.turn_running() && t.session.is_some()
        })
    });
    ask(&thread, "/btw what is it", cx);
    wait_until(cx, "the side chat", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    wait_until(cx, "its answer", |cx| {
        side.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
    });

    // New Side Chat inside a side chat does nothing (the palette doesn't offer it there either).
    side.update_in(cx, |v, window, cx| {
        v.new_side_chat(&NewSideChat, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(ws.read_with(cx, |ws, _| ws.side_chats.len()), 1);

    // The parent closes (archived, deleted): its side chat stops and its pane goes.
    ws.update_in(cx, |ws, window, cx| ws.close_thread(&thread, window, cx));
    cx.run_until_parked();
    assert!(ws.read_with(cx, |ws, _| ws.side_chats.is_empty()));
    assert!(
        side.read_with(cx, |v, _| v.session.is_none()),
        "its agent was stopped"
    );
}
