//! Commit with a staged deletion among the listed paths (what `diff HEAD` lists after `git rm`).
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let s = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .expect("git");
    assert!(s.success(), "git {args:?}");
}

#[test]
fn commit_all_with_a_staged_deletion_listed() {
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
    std::fs::write(repo.join("b.txt"), "b\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["rm", "-q", "a.txt"]);
    std::fs::write(repo.join("b.txt"), "c\n").expect("w");
    let r = skyelab_git::commit(
        &repo,
        "m",
        &[PathBuf::from("a.txt"), PathBuf::from("b.txt")],
        &[],
        None,
    );
    assert!(r.is_ok(), "COMMIT_ALL_FAILED: {r:?}");
}
