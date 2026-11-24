//! Discarding a worktree never drops changes the user wasn't shown.

use super::changes_model_test::{git, repo};
use super::worktree::{Discard, discard};

#[test]
fn discard_refuses_dirty_until_confirmed_with_the_current_status() {
    let (_tmp, root) = repo(&[("a.txt", b"a\n")]);
    let wt = root.join(".skyelab/worktrees/w");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "skyelab/w",
            skyelab_core::files::without_verbatim(&wt).to_str().expect("utf-8"),
            "HEAD",
        ],
    );
    std::fs::write(wt.join("a.txt"), "changed\n").expect("dirty");

    let Discard::NeedsConfirm(shown) = discard(&wt, None).expect("discard") else {
        panic!("a dirty worktree was removed without asking");
    };
    assert!(wt.exists());

    // More changes appeared after the prompt: the old confirm no longer covers them.
    std::fs::write(wt.join("new.txt"), "more\n").expect("more");
    let Discard::NeedsConfirm(now) = discard(&wt, Some(&shown)).expect("discard") else {
        panic!("a stale confirm removed changes it never showed");
    };
    assert!(wt.exists());
    assert_ne!(now, shown);

    assert!(matches!(
        discard(&wt, Some(&now)).expect("discard"),
        Discard::Removed
    ));
    assert!(!wt.exists());
    // The branch stays, so the work can still be recovered.
    git(&root, &["rev-parse", "--verify", "refs/heads/skyelab/w"]);
}

#[test]
fn a_clean_worktree_is_removed_at_once() {
    let (_tmp, root) = repo(&[("a.txt", b"a\n")]);
    let wt = root.join("wt");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "skyelab/c",
            skyelab_core::files::without_verbatim(&wt).to_str().expect("utf-8"),
            "HEAD",
        ],
    );
    assert!(matches!(
        discard(&wt, None).expect("discard"),
        Discard::Removed
    ));
    assert!(!wt.exists());
}
