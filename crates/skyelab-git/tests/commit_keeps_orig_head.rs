//! After a commit-all that ends a merge, ORIG_HEAD still names the commit before it.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "one\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo)
}

#[test]
fn commit_all_ending_a_merge_keeps_orig_head() {
    let (_tmp, repo) = repo();
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("b.txt"), "side\n").expect("w");
    git(&repo, &["add", "b.txt"]);
    git(&repo, &["commit", "-q", "-m", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("c.txt"), "main\n").expect("w");
    git(&repo, &["add", "c.txt"]);
    git(&repo, &["commit", "-q", "-m", "main"]);
    let before = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["merge", "-q", "--no-commit", "--no-ff", "side"]);
    git(&repo, &["update-ref", "ORIG_HEAD", &before]);
    let reviewed = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    skyelab_git::commit(&repo, "merge", &[], &[], true, Some(&reviewed)).expect("merge commit");
    assert_eq!(
        git(&repo, &["rev-parse", "ORIG_HEAD"]),
        before,
        "ORIG_HEAD_LOST"
    );
}
