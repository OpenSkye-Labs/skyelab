//! A ticked file renamed in an earlier commit (base mode lists its old path too) still commits.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
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
}

#[test]
fn ticking_a_file_renamed_in_an_earlier_commit_commits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = std::fs::canonicalize(tmp.path()).expect("canonical");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("old.txt"), "one\ntwo\nthree\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    git(&repo, &["mv", "old.txt", "new.txt"]);
    git(&repo, &["commit", "-q", "-m", "rename"]);
    std::fs::write(repo.join("new.txt"), "one\ntwo\nthree\nfour\n").expect("edit");
    let r = skyelab_git::commit(
        &repo,
        "c",
        &[PathBuf::from("new.txt"), PathBuf::from("old.txt")],
        &[],
        None,
    );
    assert!(r.is_ok(), "RENAME_PATHSPEC: {r:?}");
}
