//! `start_thread` answers with the child's id as soon as its row exists, not once its agent is up, so
//! a slow or failed spawn can't leave a child the parent never heard of.

use super::delegation_test::{host_call, open, rig, start, thread_id};
use super::*;
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};

#[gpui::test]
fn the_reply_names_the_child_even_when_its_agent_fails_to_start(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    // Children launched from now on spawn a binary that isn't there: the row is saved, the spawn fails.
    ws.update(cx, |ws, _| {
        ws.backend.claude_bin = rig_missing_bin();
    });
    let reply = host_call(
        cx,
        Caller {
            thread_id: parent_id,
            account_id: "work".into(),
        },
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "anything".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect("the row exists, so the start answers with it");
    let id = reply["thread_id"].as_i64().expect("thread_id");
    let row = rig
        .store
        .lock()
        .expect("lock")
        .thread(id)
        .expect("read")
        .expect("the child row");
    assert_eq!(row.parent_thread_id, Some(parent_id));
    assert_eq!(row.status, "failed");
}

fn rig_missing_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/no-such-agent")
}
