//! A retry with the same request id after the child's tab was closed still names that child, not a new one.

use super::delegation_test::{host_call, open, rig, start, thread_id};
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn a_retry_after_the_child_tab_closed_returns_the_same_child(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let call = || Tool::StartThread {
        project_id: rig.work_b,
        prompt: "hold".into(),
        worktree: false,
        request_id: Some("r1".into()),
    };
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let first = host_call(cx, caller(), call()).expect("first")["thread_id"]
        .as_i64()
        .expect("id");
    ws.update_in(cx, |w, win, cx| {
        let v = w.open_view(first, cx).expect("child tab");
        w.dock.update(cx, |d, cx| d.remove_panel(v, win, cx));
    });
    cx.run_until_parked();
    let again = host_call(cx, caller(), call()).expect("retry")["thread_id"]
        .as_i64()
        .expect("id");
    assert_eq!(first, again, "retry created a second child");
}
