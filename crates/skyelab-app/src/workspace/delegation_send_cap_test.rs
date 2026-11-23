//! Audit #4: a message that gives an idle child a new turn takes a child slot, and the parent's agent is told
//! when there is none: an error, not a quiet "ok".

use super::delegation_test::{host_call, open, rig, start, statuses, thread_id, wait_until};
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn a_message_to_an_idle_child_past_the_cap_is_refused(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let store = rig.store.clone();
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let start_tool = |p: &str| Tool::StartThread {
        project_id: rig.work_b,
        prompt: p.into(),
        worktree: false,
        request_id: None,
    };
    for n in 0..4 {
        host_call(cx, caller(), start_tool(&format!("child {n}"))).expect("under the limit");
    }
    wait_until(cx, "four idle children", |_| {
        let s = statuses(&store, parent_id);
        s.len() == 4 && s.iter().all(|s| s == "idle")
    });
    for _ in 0..4 {
        host_call(cx, caller(), start_tool("hold")).expect("idle children don't count");
    }
    let idle = store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids")
        .into_iter()
        .find(|t| t.status == "idle")
        .expect("an idle child");
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
    let before = prompts(idle.id);

    let sent = host_call(
        cx,
        caller(),
        Tool::SendMessage {
            thread_id: idle.id,
            text: "one more turn".into(),
        },
    );
    let err = sent.expect_err("a fifth busy child");
    assert!(err.contains("running children"), "{err}");
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let row = store
        .lock()
        .expect("lock")
        .thread(idle.id)
        .expect("read")
        .expect("row");
    assert_eq!(row.status, "idle", "the child must stay idle");
    assert_eq!(prompts(idle.id), before, "no prompt stored");
}
