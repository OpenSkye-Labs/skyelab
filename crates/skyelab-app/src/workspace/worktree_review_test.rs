//! Review fixes: a repo with no commits, a worktree deleted by hand, the slug, the confirm's file list.

use super::changes_model::{LineKind, Row, load};
use super::changes_model_test::git;
use super::worktree::{Discard, changed_files, discard, slug};
use std::path::Path;

#[test]
fn changes_of_a_repo_without_commits_lists_new_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    git(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("staged.txt"), "a\n").expect("write");
    git(&root, &["add", "staged.txt"]);
    std::fs::write(root.join("loose.txt"), "b\n").expect("write");

    let c = load(&root, None).expect("an unborn HEAD still loads");
    let paths: Vec<&Path> = c.files.iter().map(|f| f.path.as_path()).collect();
    assert_eq!(paths, [Path::new("loose.txt"), Path::new("staged.txt")]);
    assert!(c.rows.contains(&Row::Line {
        old: None,
        new: Some(1),
        kind: LineKind::Added,
        text: "a".into()
    }));
}

#[test]
fn a_worktree_deleted_by_hand_discards_cleanly() {
    let (_tmp, root) = super::changes_model_test::repo(&[("a.txt", b"a\n")]);
    let wt = root.join("wt");
    let wt_arg = wt.to_str().expect("utf-8");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "skyelab/gone",
            wt_arg,
            "HEAD",
        ],
    );
    std::fs::remove_dir_all(&wt).expect("delete by hand");
    assert!(matches!(
        discard(&wt, None).expect("discard"),
        Discard::Removed
    ));
}

#[test]
fn slug_is_lowercase_words_and_a_five_char_suffix() {
    assert_eq!(slug("Fix the login bug!", 0), "fix-the-login-bug-00000");
    assert_eq!(slug("¿¿", 35), "thread-0000z");
    let long = slug("a very long title that keeps going and going", 0);
    assert_eq!(long, "a-very-long-title-that-k-00000");
}

#[test]
fn the_confirm_names_each_changed_file_once() {
    let (_tmp, root) = super::changes_model_test::repo(&[("a.txt", b"a\n")]);
    std::fs::write(root.join("a.txt"), "changed\n").expect("modify");
    git(&root, &["add", "a.txt"]);
    std::fs::write(root.join("a.txt"), "again\n").expect("modify again");
    std::fs::write(root.join("new.txt"), "n\n").expect("new");
    let st = skyelab_git::status(&root).expect("status");
    assert_eq!(
        changed_files(&st),
        [Path::new("a.txt"), Path::new("new.txt")]
    );
}
