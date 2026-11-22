//! A Changes tab refreshes when any thread of its folder finishes a turn, not only the threads
//! that pressed "Changes".

use super::thread::ThreadViewEvent;
use super::worktree_thread_test::{fixture, open_thread, wait_until};

#[gpui::test]
fn changes_tab_refreshes_for_a_folder_thread_that_never_opened_it(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, first, cx) = open_thread(&f, cx);
    cx.dispatch_action(super::NewThread);
    cx.run_until_parked();
    let second = ws
        .read_with(&*cx, |ws, _| {
            ws.open_threads.last().and_then(|t| t.upgrade())
        })
        .expect("second thread tab");
    assert_ne!(first, second);

    first.update(cx, |_, cx| cx.emit(ThreadViewEvent::OpenChanges));
    cx.run_until_parked();
    let view = ws
        .read_with(&*cx, |ws, _| {
            ws.changes_views.last().and_then(|c| c.upgrade())
        })
        .expect("changes tab");
    wait_until(cx, "first load", |cx| {
        view.read_with(cx, |c, _| c.changes.is_some())
    });

    std::fs::write(f.project.join("second.txt"), "from the second thread\n").expect("write");
    second.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    wait_until(cx, "reload on the second thread's TurnDone", |cx| {
        view.read_with(cx, |c, _| {
            c.changes.as_ref().is_some_and(|c| c.files.len() == 1)
        })
    });
}
