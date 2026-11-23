//! N6: a commit-all that fails (here a pre-commit hook refuses it) leaves the user's index exactly as it was:
//! a partial staging is not turned into "everything staged".
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
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
fn a_failed_commit_all_restores_the_index_exactly() {
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

    // Staged: a.txt's first edit. Not staged: its second edit, b.txt's edit, an untracked file.
    std::fs::write(repo.join("a.txt"), "two\n").expect("w");
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("a.txt"), "three\n").expect("w");
    std::fs::write(repo.join("b.txt"), "bb\n").expect("w");
    std::fs::write(repo.join("new.txt"), "new\n").expect("w");
    let hook = repo.join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho 'refused by the hook' >&2\nexit 1\n").expect("hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let index = |repo: &Path| {
        (
            git(repo, &["ls-files", "--stage"]),
            git(repo, &["diff", "--cached", "--name-status"]),
        )
    };
    let before = index(&repo);
    let reviewed = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    let err = skyelab_git::commit(&repo, "all of it", &[], &[], Some(&reviewed))
        .expect_err("the hook refuses");
    assert!(format!("{err}").contains("refused by the hook"), "{err}");
    assert_eq!(index(&repo), before, "the index is what the user staged");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]).trim(), "1");
}

#[test]
fn a_commit_all_refused_for_changed_files_leaves_the_index_exactly() {
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
    git(&repo, &["add", "a.txt"]);
    std::fs::write(repo.join("a.txt"), "three\n").expect("w");
    let index = |repo: &Path| {
        (
            git(repo, &["ls-files", "--stage"]),
            git(repo, &["diff", "--cached", "--name-status"]),
        )
    };
    let before = index(&repo);
    let reviewed = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    std::fs::write(repo.join("a.txt"), "four\n").expect("w");
    let err = skyelab_git::commit(&repo, "all", &[], &[], Some(&reviewed)).expect_err("changed");
    assert!(
        format!("{err}").contains("changed since you looked"),
        "{err}"
    );
    assert_eq!(index(&repo), before);
}
