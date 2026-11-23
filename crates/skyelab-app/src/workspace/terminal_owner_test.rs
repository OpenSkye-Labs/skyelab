//! Acceptance: a terminal tab belongs to the thread shown when it opened. Another thread hides it (the shell keeps
//! running) and its own thread shows the same terminal again.

use super::center::Page;
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use super::{NewTerminal, NewThread};

#[gpui::test]
fn terminals_follow_their_thread(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, first, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let mine = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let shell = mine.read_with(cx, |p, _| p.panes[0].view.entity_id());

    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    assert!(
        !mine.read_with(cx, |p, _| p.docked),
        "another thread hides it"
    );
    assert!(
        ws.read_with(cx, |ws, _| ws.terminals_empty.is_some()),
        "the placeholder shows"
    );

    cx.dispatch_action(NewTerminal);
    wait_until(cx, "the second thread's terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 2)
    });
    let theirs = ws.read_with(cx, |ws, _| ws.terminals[1].clone());

    ws.update_in(cx, |ws, window, cx| {
        ws.show(Page::Thread(first.downgrade()), window, cx)
    });
    cx.run_until_parked();
    assert!(
        mine.read_with(cx, |p, _| p.docked),
        "its own thread shows it again"
    );
    assert!(!theirs.read_with(cx, |p, _| p.docked));
    assert_eq!(
        mine.read_with(cx, |p, _| p.panes[0].view.entity_id()),
        shell,
        "the same shell"
    );
}

#[gpui::test]
fn a_closed_threads_terminals_show_with_every_thread(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, first, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let mine = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    assert!(!mine.read_with(cx, |p, _| p.docked));

    ws.update_in(cx, |ws, window, cx| ws.close_thread(&first, window, cx));
    cx.run_until_parked();
    assert!(
        mine.read_with(cx, |p, _| p.docked),
        "not lost with its thread"
    );
    assert_eq!(mine.read_with(cx, |p, _| p.owner), None);
}
