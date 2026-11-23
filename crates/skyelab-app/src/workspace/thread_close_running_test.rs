//! Closing the tab of a running thread ends its agent and leaves the row `stopped`.

use super::thread::ThreadView;
use super::thread_close_starting_test::{backend, rig, row, wait_until};
use gpui::TestAppContext;

#[gpui::test]
fn a_tab_closed_while_running_leaves_its_thread_stopped(cx: &mut TestAppContext) {
    let r = rig(cx);
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, cx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(cx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    wait_until(cx, "the agent to run", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
            && row(&r).is_some_and(|t| t.status == "running")
    });
    drop(view);
    cx.update(|window, _| window.remove_window());
    wait_until(cx, "the row to say stopped", |_| {
        row(&r).is_some_and(|t| t.status == "stopped")
    });
}
