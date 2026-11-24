//! A fork made in its source's worktree shares that worktree's columns, and a discard can see when another thread
//! sharing it is busy.

use skyelab_store::{NewThread, Store};
use std::path::Path;

#[test]
fn a_fork_in_the_sources_worktree_shares_it_and_its_busy_state_shows() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let wt = Path::new("/work/demo/.wt/fix");
    let new = |cwd| NewThread {
        project_id: p.id,
        title: "fix",
        cwd,
        account_id: "work",
        status: "idle",
    };
    let src = s.create_thread(&new(wt)).expect("thread").id;
    s.set_thread_worktree(src, Some(wt), Some("fix"))
        .expect("worktree");
    s.set_thread_worktree_base(src, Some("abc")).expect("base");

    let shared = s.create_fork(&new(wt), src, "ctx", &[]).expect("fork");
    assert_eq!(shared.worktree_path.as_deref(), Some(wt));
    assert_eq!(shared.worktree_branch.as_deref(), Some("fix"));
    assert_eq!(shared.worktree_base.as_deref(), Some("abc"));
    let apart = s
        .create_fork(&new(Path::new("/work/demo")), src, "ctx", &[])
        .expect("fork");
    assert_eq!(apart.worktree_path, None, "elsewhere: no worktree");

    assert!(!s.worktree_busy_elsewhere(shared.id, wt).expect("busy"));
    s.set_thread_status(src, "running").expect("status");
    assert!(s.worktree_busy_elsewhere(shared.id, wt).expect("busy"));
    assert!(
        !s.worktree_busy_elsewhere(src, wt)
            .expect("itself doesn't count")
    );
}
