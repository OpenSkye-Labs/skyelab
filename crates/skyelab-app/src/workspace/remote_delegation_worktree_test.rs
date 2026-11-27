//! Criterion 15: a remote start in a new worktree creates it but runs no setup script (it runs repo code in a
//! terminal with the user's ssh agent), and the child never gets Full access.

use super::remote_delegation_harness::*;
use super::*;
use crate::test_wait::wait_until;
use gpui::TestAppContext;
use skyelab_protocol::DelegateOp;

#[gpui::test]
fn a_remote_worktree_start_runs_no_setup_script(cx: &mut TestAppContext) {
    let (b, cx) = b_side(
        Setup {
            link_to: Some("work"),
            repo: true,
            ..Setup::default()
        },
        cx,
    );
    let project = b.project("work-a");
    lock(&b.store)
        .and_then(|s| s.add_project_script(project, "Setup", "touch setup-ran.txt", true))
        .expect("script");
    let id = started(
        &b,
        &caller(),
        DelegateOp::Start {
            project_id: project,
            prompt: "in a worktree".into(),
            worktree: true,
            request_id: "r1".into(),
        },
        cx,
    );
    wait_for_agent(&b, id, cx);
    let row = lock(&b.store).and_then(|s| s.thread(id)).expect("read").expect("row");
    let wt = row.worktree_path.clone().expect("a worktree");
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!wt.join("setup-ran.txt").exists());
    b.ws.read_with(cx, |ws, _| assert!(ws.terminals.is_empty(), "no setup terminal"));
    assert_ne!(row.permission_mode, "bypassPermissions");
    let view = b.ws.read_with(cx, |ws, cx| ws.open_view(id, cx)).expect("tab");
    wait_until(cx, "the view", |cx| view.read_with(cx, |v, _| !v.starting));
    view.read_with(cx, |v, _| assert_ne!(v.mode, skyelab_agent::Mode::Bypass));
}
