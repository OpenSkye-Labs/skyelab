//! Commit all (no paths, a reviewed tree): commits exactly the reviewed tree, including staged deletions and a
//! resolved merge, and refuses anything else.
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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
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
    (tmp, repo)
}

#[test]
fn commit_all_records_a_staged_deletion_and_a_rename() {
    let (_tmp, repo) = repo();
    git(&repo, &["rm", "-q", "a.txt"]);
    git(&repo, &["mv", "b.txt", "c.txt"]);
    let tree = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    skyelab_git::commit(&repo, "m", &[], &[], true, Some(&tree)).expect("commit all");
    assert_eq!(git(&repo, &["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]),
        "c.txt"
    );
}

#[test]
fn commit_all_finishes_a_resolved_merge() {
    let (_tmp, repo) = repo();
    git(&repo, &["checkout", "-q", "-b", "side"]);
    std::fs::write(repo.join("a.txt"), "side\n").expect("w");
    git(&repo, &["commit", "-q", "-am", "side"]);
    git(&repo, &["checkout", "-q", "main"]);
    std::fs::write(repo.join("a.txt"), "main\n").expect("w");
    git(&repo, &["commit", "-q", "-am", "main"]);
    let _ = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["merge", "-q", "side"])
        .output();
    std::fs::write(repo.join("a.txt"), "both\n").expect("resolve");
    let tree = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    skyelab_git::commit(&repo, "merge", &[], &[], true, Some(&tree)).expect("commit the merge");
    assert_eq!(
        git(&repo, &["rev-list", "--count", "--merges", "HEAD"]),
        "1"
    );
}

#[test]
fn commit_all_refuses_a_file_that_appeared_after_review() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("a.txt"), "two\n").expect("w");
    let tree = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    let before = git(&repo, &["rev-parse", "HEAD"]);
    std::fs::write(repo.join("late.env"), "SECRET\n").expect("late");
    let err = skyelab_git::commit(&repo, "m", &[], &[], true, Some(&tree)).expect_err("refuse");
    assert!(err.to_string().contains("changed since"), "{err}");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), before);
}
