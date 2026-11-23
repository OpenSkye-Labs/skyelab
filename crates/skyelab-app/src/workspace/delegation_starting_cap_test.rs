//! A child whose launch is still in progress (row `starting`) holds its slot like a running one.

use super::delegation_test::{host_call, open, rig, start, thread_id};
use gpui::TestAppContext;
use skyelab_agent::mcp::{Caller, Tool};
use skyelab_core::delegation::MAX_CHILDREN;

#[gpui::test]
fn starting_children_count_toward_the_cap(cx: &mut TestAppContext) {
    let (rig, accounts, config) = rig();
    let (ws, cx) = open(&rig, accounts, config, cx);
    let parent = start(&ws, rig.work_a, "plain work", cx);
    let parent_id = thread_id(&parent, cx);
    let cwd = rig
        .store
        .lock()
        .expect("lock")
        .project(rig.work_b)
        .expect("read")
        .expect("p")
        .path;
    {
        let store = rig.store.lock().expect("lock");
        for _ in 0..MAX_CHILDREN {
            let t = store
                .create_thread(&skyelab_store::NewThread {
                    project_id: rig.work_b,
                    title: "on its way",
                    cwd: &cwd,
                    account_id: "work",
                    status: "starting",
                })
                .expect("row");
            store
                .set_thread_parent(t.id, Some(parent_id))
                .expect("link");
        }
    }
    let err = host_call(
        cx,
        Caller {
            thread_id: parent_id,
            account_id: "work".into(),
        },
        Tool::StartThread {
            project_id: rig.work_b,
            prompt: "one more".into(),
            worktree: false,
            request_id: None,
        },
    )
    .expect_err("four starting children fill the slots");
    assert!(err.contains("running children"), "{err}");
    let kids = rig
        .store
        .lock()
        .expect("lock")
        .child_threads(parent_id)
        .expect("kids");
    assert_eq!(kids.len(), MAX_CHILDREN, "no fifth row");
}
