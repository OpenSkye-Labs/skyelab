//! Audit #7: a parent whose agent was stopped, or that has no tab here any more, can't start or drive children,
//! even with a call that was already on its way when it stopped.

use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::thread::Stop;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn a_stopped_or_closed_parent_cannot_delegate(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = |id| Caller {
        thread_id: id,
        account_id: "work".into(),
    };
    let start_tool = || Tool::StartThread {
        project_id: rig.work_b,
        prompt: "hold".into(),
        worktree: false,
        request_id: None,
    };
    let child = host_call(cx, caller(parent_id), start_tool()).expect("a live parent delegates");
    let child = child["thread_id"].as_i64().expect("id");
    let prompts = |id| {
        store
            .lock()
            .expect("lock")
            .events_since(id, 0)
            .expect("events")
            .iter()
            .filter(|e| e.kind == "prompt")
            .count()
    };
    let kids = || {
        store
            .lock()
            .expect("lock")
            .child_threads(parent_id)
            .expect("kids")
            .len()
    };

    parent.update_in(cx, |v, window, cx| v.stop(&Stop, window, cx));
    wait_until(cx, "the parent stopped", |cx| {
        parent.read_with(cx, |v, _| v.session.is_none())
    });
    let err = host_call(cx, caller(parent_id), start_tool()).expect_err("stopped parent");
    assert!(
        err.contains("stopped") || err.contains("isn't running"),
        "{err}"
    );
    assert_eq!(kids(), 1, "no second child");
    let before = prompts(child);
    let send = Tool::SendMessage {
        thread_id: child,
        text: "more".into(),
    };
    host_call(cx, caller(parent_id), send).expect_err("stopped parent can't drive");
    assert_eq!(prompts(child), before, "nothing reached the child");

    // A parent with a row but no tab in this window (closed, or left over from another run).
    let orphan = {
        let s = store.lock().expect("lock");
        let cwd = s.project(rig.work_a).expect("read").expect("p").path;
        s.create_thread(&skyelab_store::NewThread {
            project_id: rig.work_a,
            title: "no tab",
            cwd: &cwd,
            account_id: "work",
            status: "running",
        })
        .expect("row")
        .id
    };
    host_call(cx, caller(orphan), start_tool()).expect_err("a parent without a tab");
    assert!(
        store
            .lock()
            .expect("lock")
            .child_threads(orphan)
            .expect("kids")
            .is_empty()
    );
}
