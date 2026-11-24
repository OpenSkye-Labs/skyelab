//! Branch list, create and switch on temp repos: a dirty tree is never switched without a choice, and no choice
//! loses work.

use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{Error, OnDirty, branches, create_branch, switch_branch};

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

/// A repo on `main` with `a.txt`, plus a `feature` branch where `a.txt` differs.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
        // Git for Windows defaults to autocrlf; these tests read back files git writes.
        ("core.autocrlf", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "main\n").expect("w");
    std::fs::write(repo.join("b.txt"), "b\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["switch", "-q", "-c", "feature"]);
    std::fs::write(repo.join("a.txt"), "feature\n").expect("w");
    git(&repo, &["commit", "-q", "-am", "feature"]);
    git(&repo, &["switch", "-q", "main"]);
    (tmp, repo)
}

fn current(repo: &Path) -> String {
    git(repo, &["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_owned()
}

#[test]
fn lists_local_branches_and_the_current_one() {
    let (_tmp, repo) = repo();
    let b = branches(&repo).expect("branches");
    assert_eq!(b.current.as_deref(), Some("main"));
    let mut local = b.local.clone();
    local.sort();
    assert_eq!(local, ["feature", "main"]);
    assert!(b.elsewhere.is_empty());
    git(&repo, &["switch", "-q", "--detach", "HEAD"]);
    assert_eq!(branches(&repo).expect("branches").current, None);
}

#[test]
fn names_branches_checked_out_in_another_worktree() {
    let (tmp, repo) = repo();
    let other = tmp.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", &other.to_string_lossy(), "feature"],
    );
    assert_eq!(branches(&repo).expect("branches").elsewhere, ["feature"]);
    // From the worktree, main is the one checked out elsewhere.
    assert_eq!(branches(&other).expect("branches").elsewhere, ["main"]);
}

#[test]
fn clean_switch_and_create() {
    let (_tmp, repo) = repo();
    switch_branch(&repo, "feature", OnDirty::Refuse).expect("clean switch");
    assert_eq!(current(&repo), "feature");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("r"),
        "feature\n"
    );
    create_branch(&repo, "topic/new").expect("create");
    assert_eq!(current(&repo), "topic/new");
    assert!(matches!(
        create_branch(&repo, "main"),
        Err(Error::BranchExists(_))
    ));
    for bad in ["-D", "a..b", "", "with space"] {
        assert!(
            matches!(create_branch(&repo, bad), Err(Error::InvalidBranch(_))),
            "{bad:?}"
        );
        assert!(
            switch_branch(&repo, bad, OnDirty::Carry).is_err(),
            "{bad:?}"
        );
    }
    assert!(switch_branch(&repo, "missing", OnDirty::Carry).is_err());
}

#[test]
fn dirty_switch_is_refused_without_a_choice() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("b.txt"), "edited\n").expect("w");
    std::fs::write(repo.join("new.txt"), "untracked\n").expect("w");
    let err = switch_branch(&repo, "feature", OnDirty::Refuse).expect_err("dirty");
    assert!(matches!(err, Error::Dirty(_)), "{err:?}");
    assert_eq!(current(&repo), "main");
    assert_eq!(
        std::fs::read_to_string(repo.join("b.txt")).expect("r"),
        "edited\n"
    );
    assert!(repo.join("new.txt").exists());
}

#[test]
fn an_ignored_file_alone_is_not_dirty_and_survives() {
    let (_tmp, repo) = repo();
    git(&repo, &["switch", "-q", "feature"]);
    std::fs::write(repo.join(".gitignore"), ".env\n").expect("w");
    git(&repo, &["add", ".gitignore"]);
    git(&repo, &["commit", "-q", "-m", "ignore"]);
    std::fs::write(repo.join(".env"), "SECRET=1\n").expect("w");
    switch_branch(&repo, "main", OnDirty::Refuse).expect("only an ignored file");
    assert_eq!(
        std::fs::read_to_string(repo.join(".env")).expect("r"),
        "SECRET=1\n"
    );
}

#[test]
fn stash_keeps_every_change_in_the_stash() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("a.txt"), "conflicting edit\n").expect("w");
    std::fs::write(repo.join("new.txt"), "untracked\n").expect("w");
    switch_branch(&repo, "feature", OnDirty::Stash).expect("stash and switch");
    assert_eq!(current(&repo), "feature");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("r"),
        "feature\n"
    );
    assert!(!repo.join("new.txt").exists());
    let list = git(&repo, &["stash", "list"]);
    assert!(list.contains("skyelab"), "{list}");
    // The work is all there: back on main, the stash applies cleanly.
    git(&repo, &["switch", "-q", "main"]);
    git(&repo, &["stash", "pop", "-q"]);
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("r"),
        "conflicting edit\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("new.txt")).expect("r"),
        "untracked\n"
    );
}

#[test]
fn carry_takes_changes_along_and_a_conflict_keeps_them() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("b.txt"), "edited\n").expect("w");
    switch_branch(&repo, "feature", OnDirty::Carry).expect("carry");
    assert_eq!(current(&repo), "feature");
    assert_eq!(
        std::fs::read_to_string(repo.join("b.txt")).expect("r"),
        "edited\n"
    );

    // a.txt differs between the branches: carrying an edit of it would overwrite it, so git refuses.
    std::fs::write(repo.join("a.txt"), "mine\n").expect("w");
    switch_branch(&repo, "main", OnDirty::Carry).expect_err("conflict");
    assert_eq!(current(&repo), "feature");
    assert_eq!(
        std::fs::read_to_string(repo.join("a.txt")).expect("r"),
        "mine\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("b.txt")).expect("r"),
        "edited\n"
    );
}

#[test]
fn a_switch_never_overwrites_an_ignored_file() {
    let (_tmp, repo) = repo();
    // `feature` tracks notes.txt; on main it is ignored and holds the user's own text.
    git(&repo, &["switch", "-q", "feature"]);
    std::fs::write(repo.join("notes.txt"), "tracked\n").expect("w");
    git(&repo, &["add", "notes.txt"]);
    git(&repo, &["commit", "-q", "-m", "notes"]);
    git(&repo, &["switch", "-q", "main"]);
    std::fs::write(repo.join(".git/info/exclude"), "notes.txt\n").expect("w");
    std::fs::write(repo.join("notes.txt"), "mine\n").expect("w");
    assert!(switch_branch(&repo, "feature", OnDirty::Refuse).is_err());
    assert_eq!(current(&repo), "main");
    assert_eq!(
        std::fs::read_to_string(repo.join("notes.txt")).expect("r"),
        "mine\n"
    );
}

/// Git's own "checked out in another worktree" check reads the common dir's worktree list, which a switch on
/// frozen config doesn't see: SkyeLab checks first.
#[test]
fn never_switches_to_a_branch_another_worktree_has() {
    let (_tmp, repo) = repo();
    let elsewhere = tempfile::TempDir::new().expect("tempdir");
    let other = elsewhere.path().join("wt");
    git(
        &repo,
        &["worktree", "add", "-q", &other.to_string_lossy(), "feature"],
    );
    let err = switch_branch(&repo, "feature", OnDirty::Refuse).expect_err("refused");
    assert!(err.to_string().contains("another worktree"), "{err}");
    assert_eq!(current(&repo), "main");
}
