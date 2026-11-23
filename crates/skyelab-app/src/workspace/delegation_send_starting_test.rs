//! A send to a child whose launch is still running says it is starting, not that it ended.

use super::delegation_test::{host_call, open, rig, start, thread_id, wait_until};
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn a_send_to_a_starting_child_says_it_is_starting(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let caller = || Caller {
        thread_id: parent_id,
        account_id: "work".into(),
    };
    let reply = host_call(
        cx,
        caller(),
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "hold".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("started");
    let child = reply["thread_id"].as_i64().expect("thread_id");
    let view = ws
        .read_with(cx, |ws, cx| ws.open_view(child, cx))
        .expect("child tab");
    wait_until(cx, "the child's launch to end", |cx| {
        view.read_with(cx, |v, _| v.session.is_some())
    });
    // What the early row leaves while the agent spawns: the row is known, the launch hasn't ended.
    view.update(cx, |v, _| v.starting = true);
    let err = host_call(
        cx,
        caller(),
        Tool::SendMessage {
            thread_id: child,
            text: "hi".into(),
        },
    )
    .expect_err("refused while starting");
    assert!(err.contains("still starting"), "{err}");
}
