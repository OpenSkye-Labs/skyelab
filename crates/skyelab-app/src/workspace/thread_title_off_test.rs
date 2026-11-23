//! Acceptance: with "Generate thread titles" off, no title call runs and the first-line title stays.

use super::thread_title_test::titles;
use crate::workspace::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::TestAppContext;

#[gpui::test]
fn titles_off_runs_no_title_call(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    titles(false, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    std::thread::sleep(std::time::Duration::from_millis(500));
    cx.run_until_parked();
    assert!(
        !f.project_dir.join(".fake-title-call").exists(),
        "no title process ran"
    );
    assert_eq!(thread.read_with(cx, |t, _| t.title()).as_ref(), "hi");
}
