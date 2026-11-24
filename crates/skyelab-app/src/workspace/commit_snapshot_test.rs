//! Acceptance: Commit commits only what the Changes tab showed. Any change on disk after it loaded refuses the
//! commit (new files, edits, an untracked file too big for the tab to show), and the button names the count.

use super::changes_model::load;
use super::changes_model_test::{git, repo};
use super::changes_publish::commit_label;
use std::path::{Path, PathBuf};
use std::process::Command;

fn head(root: &Path) -> String {
    let out = skyelab_core::process::output(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"]),
        None,
    )
    .expect("git");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn listed(root: &Path) -> (Vec<PathBuf>, String) {
    let c = load(root, None).expect("load");
    let paths = c.files.iter().map(|f| f.path.clone()).collect();
    (paths, c.tree.expect("load records the tree it showed"))
}

fn refuses(root: &Path, paths: &[PathBuf], tree: &str) {
    let before = head(root);
    let err = skyelab_git::commit(root, "c", paths, &[], true, Some(tree)).expect_err("must refuse");
    assert!(err.to_string().contains("changed since"), "{err}");
    assert_eq!(head(root), before, "something was committed");
}

#[test]
fn a_file_that_appears_after_review_refuses_commit_all() {
    let (_tmp, root) = repo(&[("a.txt", b"one\n")]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    std::fs::write(root.join("a.txt"), "two\n").expect("edit");
    let (paths, tree) = listed(&root);
    std::fs::write(root.join(".env.local"), "SECRET=1\n").expect("late file");
    refuses(&root, &paths, &tree);
}

#[test]
fn an_edit_after_review_refuses_even_past_the_display_cap() {
    let (_tmp, root) = repo(&[("a.txt", b"one\n")]);
    let big = vec![b'x'; 2 << 20];
    std::fs::write(root.join("big.txt"), &big).expect("big");
    let (paths, tree) = listed(&root);
    let mut changed = big.clone();
    changed[(2 << 20) - 1] = b'y'; // past what the tab reads of an untracked file
    std::fs::write(root.join("big.txt"), &changed).expect("edit big");
    refuses(&root, &paths, &tree);

    let (_tmp, root) = repo(&[("a.txt", b"one\n")]);
    std::fs::write(root.join("a.txt"), "two\n").expect("edit");
    let (paths, tree) = listed(&root);
    std::fs::write(root.join("a.txt"), "three\n").expect("edit again");
    refuses(&root, &paths, &tree);
}

#[test]
fn an_unchanged_tree_commits_exactly_the_listed_files() {
    let (_tmp, root) = repo(&[("a.txt", b"one\n"), ("b.txt", b"b\n")]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    std::fs::write(root.join("a.txt"), "two\n").expect("edit");
    std::fs::write(root.join("new.txt"), "n\n").expect("new");
    let (paths, tree) = listed(&root);
    skyelab_git::commit(&root, "c", &paths, &[], true, Some(&tree)).expect("commit");
    let out = skyelab_core::process::output(
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["show", "--name-only", "--format=", "HEAD"]),
        None,
    )
    .expect("git");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a.txt\nnew.txt\n");
}

#[test]
fn the_button_names_what_will_be_committed() {
    assert_eq!(commit_label(0, 0), "Nothing to commit");
    assert_eq!(commit_label(0, 3), "Commit all 3 files");
    assert_eq!(commit_label(0, 1), "Commit all 1 file");
    assert_eq!(commit_label(1, 3), "Commit 1 file");
    assert_eq!(commit_label(2, 3), "Commit 2 files");
}
