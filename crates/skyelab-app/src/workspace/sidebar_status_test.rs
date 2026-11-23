//! The sidebar shows an open thread's live state, not its row as it was when the thread started.

use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use crate::ui::Status;
use gpui::TestAppContext;

#[gpui::test]
fn sidebar_thread_status_follows_the_store(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    wait_until(cx, "the store row settled", |_| {
        f.store
            .lock()
            .expect("lock")
            .thread(id)
            .expect("read")
            .is_some_and(|t| t.status != "starting" && t.status != "running")
    });
    cx.run_until_parked();
    let shown = ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).sections[1].items[0].status);
    assert!(
        !matches!(shown, Some(Status::Starting | Status::Running)),
        "SIDEBAR_STALE_STATUS {shown:?}"
    );
}
