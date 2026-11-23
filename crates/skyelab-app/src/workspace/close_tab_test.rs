//! Closing a local thread's view (archive or delete, through `Workspace::close_thread`) ends its agent and records
//! the thread as stopped.
#![cfg(unix)] // the fake agent is a shell script

use super::threads_test::resume::env_reports;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done, wait_until,
};
use gpui::TestAppContext;

fn alive(pid: i64) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill runs")
        .success()
}

#[gpui::test]
fn closing_a_thread_tab_kills_its_agent_and_marks_it_stopped(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    wait_until(cx, "the fake's env report", |_| {
        !env_reports(&f.store, id).is_empty()
    });
    let pid = env_reports(&f.store, id)[0]["pid"]
        .as_i64()
        .expect("the fake reports its pid");
    assert!(
        alive(pid),
        "precondition: the agent still runs between turns"
    );

    ws.update_in(cx, |ws, window, cx| {
        ws.close_thread(&thread, window, cx);
    });
    drop(thread);
    wait_until(cx, "the agent gone and the row stopped", |_| {
        let status = f
            .store
            .lock()
            .expect("lock")
            .thread(id)
            .expect("read")
            .expect("row")
            .status;
        !alive(pid) && status == "stopped"
    });
}
