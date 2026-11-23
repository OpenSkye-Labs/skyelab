//! Acceptance: every column of one multi-model send starts its worktree from the same commit. The first column to get
//! there resolves the base and pins it; the others reuse it even when the base branch moved in between.

use super::changes_model_test::git;
use super::worktree::{self, BasePin};

#[test]
fn columns_sharing_a_pin_start_from_one_commit() {
    let f = super::worktree_thread_test::fixture();
    let pin = BasePin::default();
    let first =
        worktree::create(&f.config, &f.project, "acme", "first", None, Some(&pin)).expect("first");
    // The branch moves before the next column makes its worktree.
    std::fs::write(f.project.join("README.md"), "moved\n").expect("write");
    git(&f.project, &["commit", "-q", "-am", "moved"]);
    let second = worktree::create(&f.config, &f.project, "acme", "second", None, Some(&pin))
        .expect("second");
    assert_eq!(
        first.base, second.base,
        "the columns started from different commits"
    );
    // Without a pin, a new worktree takes the branch as it is now.
    let alone =
        worktree::create(&f.config, &f.project, "acme", "alone", None, None).expect("alone");
    assert_ne!(alone.base, first.base);
}
