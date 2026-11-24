//! A worktree that wouldn't run under the project's account never starts an agent.

use super::changes_model_test::git;
use super::thread_worktree::ToggleWorktree;
use super::worktree_thread_test::{Fixture, fixture, open_thread, send, wait_until};
use gpui::TestAppContext;
use skyelab_core::Binding;

/// Sends a worktree thread's first prompt and asserts it was refused before any agent ran.
fn assert_refused(f: &Fixture, cx: &mut TestAppContext, why: &str) {
    let (_ws, thread, cx) = open_thread(f, cx);
    cx.dispatch_action(ToggleWorktree);
    send(cx);
    wait_until(cx, "the refusal", |cx| {
        thread.read_with(cx, |t, _| t.error.is_some() || t.thread.is_some())
    });
    thread.read_with(cx, |t, _| {
        let error = t.error.as_deref().unwrap_or_default();
        assert!(error.contains(why), "error: {error}");
        assert!(
            t.thread.is_none() && t.session.is_none(),
            "no agent started"
        );
    });
    let rows = f
        .store
        .lock()
        .expect("lock")
        .list_projects()
        .expect("projects");
    let threads = f
        .store
        .lock()
        .expect("lock")
        .list_threads(rows[0].id)
        .expect("threads");
    assert!(threads.is_empty(), "no thread row was written");
}

#[cfg(unix)] // the link is a unix symlink
#[gpui::test]
fn a_symlinked_skyelab_folder_is_refused(cx: &mut TestAppContext) {
    let f = fixture();
    let elsewhere = f.project.parent().expect("parent").join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("mkdir");
    std::os::unix::fs::symlink(&elsewhere, f.project.join(".skyelab")).expect("symlink");
    assert_refused(&f, cx, "not a plain folder");
    assert_eq!(
        std::fs::read_dir(&elsewhere).expect("read").count(),
        0,
        "nothing was created through the link"
    );
}

#[gpui::test]
fn worktrees_bound_to_another_account_are_refused(cx: &mut TestAppContext) {
    let mut f = fixture();
    let dir = f.project.join(".skyelab/worktrees");
    std::fs::create_dir_all(&dir).expect("mkdir");
    f.config
        .add_binding(Binding::new(dir.clone(), "other".into()))
        .expect("bind");
    assert_refused(&f, cx, "not this thread's account");
    let left: Vec<_> = std::fs::read_dir(&dir).expect("read").collect();
    assert!(
        left.is_empty(),
        "the refused worktree was removed: {left:?}"
    );
}

#[gpui::test]
fn a_project_inside_a_bigger_repo_is_refused(cx: &mut TestAppContext) {
    let mut f = fixture();
    let sub = f.project.join("sub");
    std::fs::create_dir(&sub).expect("mkdir");
    std::fs::write(sub.join("x.txt"), "x\n").expect("write");
    git(&f.project, &["add", "."]);
    git(&f.project, &["commit", "-q", "-m", "sub"]);
    f.config
        .add_binding(Binding::new(sub.clone(), "acme".into()))
        .expect("bind sub");
    f.project = sub;
    assert_refused(&f, cx, "its repo's root");
}
