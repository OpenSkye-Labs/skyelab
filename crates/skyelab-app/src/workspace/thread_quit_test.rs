//! Quitting the app ends every running agent and leaves its row `stopped`, so nothing reads as running
//! once the app is gone.

use super::thread::ThreadView;
use super::thread_close_starting_test::{backend, rig, row, wait_until};
use gpui::TestAppContext;

#[gpui::test]
fn quitting_leaves_running_threads_stopped(cx: &mut TestAppContext) {
    let r = rig(cx);
    let (config, backend, project) = (r.config.clone(), backend(&r), r.project.clone());
    let (view, vcx) =
        cx.add_window_view(|window, cx| ThreadView::new(&project, config, backend, window, cx));
    view.update_in(vcx, |v, window, cx| v.submit_remote("hold", window, cx))
        .expect("prompt taken");
    wait_until(vcx, "the agent to run", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
            && row(&r).is_some_and(|t| t.status == "running")
    });
    cx.quit();
    assert_eq!(row(&r).map(|t| t.status).as_deref(), Some("stopped"));
}
