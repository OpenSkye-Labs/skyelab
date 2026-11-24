//! `wait_thread` hands the parent the child's finished turn as data (T3's typed turn result).
#![cfg(unix)]

use super::delegation_test::{host_call, open, rig, start, statuses, thread_id, wait_until};
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn wait_thread_returns_the_childs_turn(cx: &mut gpui::TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let started = host_call(
        cx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "child".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("started");
    let child = started["thread_id"].as_i64().expect("thread id");
    wait_until(cx, "the child's turn ends", |_| {
        statuses(&store, parent_id) == ["idle"]
    });
    let waited = host_call(
        cx,
        caller(),
        Tool::WaitThread {
            thread_id: child,
            timeout_s: 5,
        },
    )
    .expect("waited");
    // The plain fields stay for older callers.
    assert_eq!(waited["thread_id"], child);
    assert_eq!(waited["done"], true);
    let turn = &waited["turn"];
    assert_eq!(turn["is_error"], false, "{waited}");
    assert_eq!(turn["summary"], "done", "{waited}");
    assert!(turn["duration_ms"].is_u64(), "{waited}");
    assert!(turn["cost_usd"].is_number(), "{waited}");
    assert!(turn["files_changed"].is_array(), "{waited}");
}
