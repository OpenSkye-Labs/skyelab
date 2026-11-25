//! Acceptance: mod+F in a side chat opens the find bar in the side chat (not the main thread beside it), counts its
//! matches, steps with Enter and closes with Esc while the main thread is on screen, also after a click on its dock tab.

use crate::workspace::thread::composer::Submit;
use crate::workspace::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::Focusable as _;

const M: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

#[gpui::test]
fn find_in_a_side_chat(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    send(cx);
    wait_until(cx, "the parent's turn ends", |cx| {
        thread.read_with(cx, |t, _| {
            t.thread.is_some() && !t.turn_running() && t.session.is_some()
        })
    });
    thread.update_in(cx, |t, window, cx| {
        t.prompt
            .update(cx, |p, cx| p.set_value("/btw what is it", window, cx));
        t.submit(Submit::Send, window, cx);
    });
    wait_until(cx, "the side chat", |cx| {
        ws.read_with(cx, |ws, _| ws.side_chats.len() == 1)
    });
    let side = ws.read_with(cx, |ws, cx| ws.side_chats[0].read(cx).view.clone());
    wait_until(cx, "its answer", |cx| {
        side.read_with(cx, |v, _| v.last_answer().is_some() && !v.turn_running())
    });
    // A click on its dock tab, the one already shown (the right dock draws last, so its tabs own these selectors):
    // the tab takes the press, and the keys must still reach the chat.
    let tab = cx.debug_bounds("dock-tab-1").expect("the side chat's tab");
    cx.simulate_click(tab.center(), gpui::Modifiers::none());
    cx.run_until_parked();
    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(
        side.read_with(cx, |v, _| v.timeline.find.input.is_some()),
        "mod+F after a tab click opens the side chat's bar"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        side.read_with(cx, |v, _| v.timeline.find.input.is_none()),
        "Esc closes it"
    );
    // F6 and the dock focus the pane by its own handle: mod+F must reach the chat in it from there too.
    let pane = ws.read_with(cx, |ws, _| ws.side_chats[0].clone());
    pane.update_in(cx, |p, window, cx| p.focus_handle(cx).focus(window, cx));
    cx.run_until_parked();

    cx.simulate_keystrokes(&format!("{M}-f"));
    cx.run_until_parked();
    assert!(
        side.read_with(cx, |v, _| v.timeline.find.input.is_some()),
        "mod+F opens the side chat's bar"
    );
    assert!(
        thread.read_with(cx, |v, _| v.timeline.find.input.is_none()),
        "not the main thread's"
    );
    // The fake's reply names "the SQLite store" once.
    cx.simulate_keystrokes("s q l i t e");
    cx.run_until_parked();
    let count = |cx: &mut gpui::VisualTestContext| {
        side.read_with(cx, |v, _| v.timeline.find.count("sqlite"))
    };
    assert_eq!(count(cx), "1/1");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(count(cx), "1/1", "Enter wraps around one match");

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        side.read_with(cx, |v, _| v.timeline.find.input.is_none()),
        "Esc closes it"
    );
}
