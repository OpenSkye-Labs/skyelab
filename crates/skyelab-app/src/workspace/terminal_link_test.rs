//! A file path cmd-clicked in a terminal reveals that file in an open Changes tab, and nothing else.

use super::thread::ThreadViewEvent;
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use skyelab_terminal::TerminalEvent;
use std::path::PathBuf;

#[gpui::test]
fn terminal_path_link_reveals_the_changed_file(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    std::fs::write(f.project.join("notes.txt"), "changed\n").expect("write");
    thread.update(cx, |_, cx| cx.emit(ThreadViewEvent::OpenChanges));
    let changes = ws
        .read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "changes load", |cx| {
        changes.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });

    cx.dispatch_action(super::NewTerminal);
    cx.run_until_parked();
    let term = ws
        .read_with(&*cx, |ws, cx| {
            ws.terminals
                .last()
                .map(|t| t.read(cx).panes[0].view.clone())
        })
        .expect("terminal");

    // An unchanged file isn't in the tab: nothing happens.
    term.update(cx, |_, cx| {
        cx.emit(TerminalEvent::OpenPath(f.project.join("README.md")))
    });
    cx.run_until_parked();
    assert_eq!(changes.read_with(cx, |c, _| c.active_file.clone()), None);

    term.update(cx, |_, cx| {
        cx.emit(TerminalEvent::OpenPath(f.project.join("notes.txt")))
    });
    cx.run_until_parked();
    assert_eq!(
        changes.read_with(cx, |c, _| c.active_file.clone()),
        Some(PathBuf::from("notes.txt"))
    );
}
