//! A `start_thread` retried with the same `request_id` (after a timeout, say) gets the child the first
//! call started, never a second one.

use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use super::orchestrate::Orchestration;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Host as _, Tool};

#[gpui::test]
fn a_retry_with_the_same_request_id_returns_the_same_child(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |id: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: "hold".into(),
        worktree: false,
        request_id: Some(id.into()),
    };
    let id_of = |v: serde_json::Value| v["thread_id"].as_i64().expect("thread_id");
    let rows = || {
        store
            .lock()
            .expect("lock")
            .child_threads(parent_id)
            .expect("kids")
            .len()
    };

    // A retry after the first answered.
    let first = id_of(host_call(cx, caller(), start_tool("r1")).expect("first"));
    let again = id_of(host_call(cx, caller(), start_tool("r1")).expect("retry"));
    assert_eq!(first, again);
    assert_eq!(rows(), 1);

    // Two calls with one id at once, so the second can arrive while the first is still on its way.
    let host = cx.update(|_, cx| cx.global::<Orchestration>().host.clone());
    let calls: Vec<_> = (0..2)
        .map(|_| {
            let (host, caller, tool) = (host.clone(), caller(), start_tool("r2"));
            std::thread::spawn(move || host.call(&caller, tool))
        })
        .collect();
    wait_until(cx, "both calls", |_| calls.iter().all(|c| c.is_finished()));
    let ids: Vec<i64> = calls
        .into_iter()
        .map(|c| id_of(c.join().expect("no panic").expect("started")))
        .collect();
    assert_eq!(ids[0], ids[1], "one child for one request id");
    assert_ne!(ids[0], first);
    assert_eq!(rows(), 2);

    // Another request id is another child.
    let third = id_of(host_call(cx, caller(), start_tool("r3")).expect("third"));
    assert!(third != first && third != ids[0]);
    assert_eq!(rows(), 3);
}
