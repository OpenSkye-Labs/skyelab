//! A commit that lands must never be reported as failed: with the real index locked, either nothing is committed
//! or the commit is returned.
#![cfg(unix)]
use std::path::Path;
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn a_locked_index_never_reports_failure_for_a_commit_that_was_made() {
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
    std::fs::write(repo.join("a.txt"), "two\n").expect("w");
    let before = git(&repo, &["rev-parse", "HEAD"]);
    let reviewed = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    std::fs::write(repo.join(".git/index.lock"), "").expect("lock");
    let result = skyelab_git::commit(&repo, "msg", &[], &[], true, Some(&reviewed));
    let after = git(&repo, &["rev-parse", "HEAD"]);
    assert!(
        result.is_ok() || before == after,
        "BUG: commit returned {result:?} but HEAD moved from {before} to {after}"
    );
}
