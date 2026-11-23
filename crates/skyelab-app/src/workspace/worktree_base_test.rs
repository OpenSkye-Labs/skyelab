//! Acceptance: a new worktree thread starts from the branch picked in the header's "From <branch>" picker, or the
//! current branch when nothing was picked, and records that commit as its worktree base. The picker on a local
//! thread switches the checkout, and asks before a dirty switch.

use super::changes_model_test::git;
use super::thread_worktree::ToggleWorktree;
use super::worktree_thread_test::{fixture, open_thread, send, wait_until};
use gpui::{TestAppContext, VisualTestContext};
use std::path::Path;

fn rev(dir: &Path, rev: &str) -> String {
    skyelab_git::rev_parse(dir, rev).expect("rev-parse")
}

fn current(dir: &Path) -> Option<String> {
    skyelab_git::branches(dir).expect("branches").current
}

/// Sends the first prompt with "Run in a new worktree" ticked and returns the stored base.
fn start_worktree_thread(
    thread: &gpui::Entity<super::thread::ThreadView>,
    pick: Option<&str>,
    cx: &mut VisualTestContext,
) -> String {
    cx.dispatch_action(ToggleWorktree);
    if let Some(b) = pick {
        let picker = thread.read_with(cx, |t, _| t.branch_picker.clone());
        wait_until(cx, "branches listed", |cx| {
            picker.read_with(cx, |p, _| !p.branches.local.is_empty())
        });
        picker.update(cx, |p, cx| p.pick(b.to_owned(), cx));
        let label = picker.read_with(cx, |p, _| p.label(true));
        assert_eq!(label, format!("From {b}"));
    }
    send(cx);
    wait_until(cx, "thread row", |cx| {
        thread.read_with(cx, |t, _| t.thread.is_some() || t.error.is_some())
    });
    thread.read_with(cx, |t, _| {
        assert_eq!(t.error, None);
        let row = t.thread.clone().expect("row");
        assert!(row.worktree_path.is_some());
        row.worktree_base.expect("base recorded")
    })
}

#[gpui::test]
fn a_picked_base_branch_is_the_worktree_base(cx: &mut TestAppContext) {
    let f = fixture();
    git(&f.project, &["switch", "-q", "-c", "feature"]);
    std::fs::write(f.project.join("feature.txt"), "f\n").expect("w");
    git(&f.project, &["add", "."]);
    git(&f.project, &["commit", "-q", "-m", "feature"]);
    git(&f.project, &["switch", "-q", "main"]);
    let (_ws, thread, cx) = open_thread(&f, cx);
    let base = start_worktree_thread(&thread, Some("feature"), cx);
    assert_eq!(base, rev(&f.project, "feature"));
    assert_ne!(base, rev(&f.project, "main"));
    assert_eq!(
        current(&f.project).as_deref(),
        Some("main"),
        "picking a base switches nothing"
    );
}

#[gpui::test]
fn with_no_pick_the_base_is_the_current_branch(cx: &mut TestAppContext) {
    let f = fixture();
    git(&f.project, &["switch", "-q", "-c", "topic"]);
    std::fs::write(f.project.join("topic.txt"), "t\n").expect("w");
    git(&f.project, &["add", "."]);
    git(&f.project, &["commit", "-q", "-m", "topic"]);
    let (_ws, thread, cx) = open_thread(&f, cx);
    let base = start_worktree_thread(&thread, None, cx);
    assert_eq!(base, rev(&f.project, "topic"));
    assert_ne!(base, rev(&f.project, "main"));
}

#[gpui::test]
fn the_picker_switches_and_asks_before_a_dirty_switch(cx: &mut TestAppContext) {
    let f = fixture();
    git(&f.project, &["branch", "other"]);
    let (_ws, thread, cx) = open_thread(&f, cx);
    let picker = thread.read_with(cx, |t, _| t.branch_picker.clone());
    wait_until(cx, "branches listed", |cx| {
        picker.read_with(cx, |p, _| p.branches.local.len() == 2)
    });
    std::fs::write(f.project.join("README.md"), "# edited\n").expect("w");
    picker.update(cx, |p, cx| p.pick("other".into(), cx));
    wait_until(cx, "the dirty prompt", |cx| {
        picker.read_with(cx, |p, _| p.dirty.is_some() || p.error.is_some())
    });
    assert_eq!(
        current(&f.project).as_deref(),
        Some("main"),
        "not switched without a choice"
    );
    // The changes moved on after the prompt showed: the confirm is refused and asked again.
    std::fs::write(f.project.join("new.txt"), "n\n").expect("w");
    let shown = picker.read_with(cx, |p, _| p.dirty.clone().expect("asked"));
    picker.update(cx, |p, cx| {
        p.switch(
            shown.0.clone(),
            skyelab_git::OnDirty::Carry,
            Some(shown.1.clone()),
            cx,
        )
    });
    wait_until(cx, "asked again", |cx| {
        picker.read_with(cx, |p, _| p.dirty.as_ref().is_some_and(|d| d.1 != shown.1))
    });
    assert_eq!(current(&f.project).as_deref(), Some("main"));
    let again = picker.read_with(cx, |p, _| p.dirty.clone().expect("asked"));
    picker.update(cx, |p, cx| {
        p.switch(again.0, skyelab_git::OnDirty::Carry, Some(again.1), cx)
    });
    wait_until(cx, "switched", |_| {
        current(&f.project).as_deref() == Some("other")
    });
    assert_eq!(
        std::fs::read_to_string(f.project.join("README.md")).expect("r"),
        "# edited\n",
        "the changes came along"
    );
    assert!(f.project.join("new.txt").exists());
}
