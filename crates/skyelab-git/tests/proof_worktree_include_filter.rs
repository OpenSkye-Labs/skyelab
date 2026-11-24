//! Proof: creating a worktree never runs a filter that only the new worktree's config (a conditional include) defines.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
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
fn create_worktree_never_runs_a_filter_only_the_new_worktree_sees() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join(".gitattributes"), "*.txt filter=wt\n").expect("attrs");
    fs::write(repo.join("a.txt"), "1\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let marker = base.join("ran");
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!("#!/bin/sh\necho smudge >> '{}'\ncat\n", marker.display()),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");
    let inc = base.join("evil.inc");
    fs::write(
        &inc,
        format!("[filter \"wt\"]\n\tsmudge = {}\n", evil.display()),
    )
    .expect("inc");
    git(
        &repo,
        &[
            "config",
            "includeIf.gitdir:**/.git/worktrees/**.path",
            inc.to_str().expect("utf-8"),
        ],
    );
    git(
        &repo,
        &[
            "config",
            "includeIf.onbranch:feat.path",
            inc.to_str().expect("utf-8"),
        ],
    );
    let dest = base.join("wt");
    skyelab_git::create_worktree(&repo, "feat", "HEAD", Some(&dest), false).expect("worktree");
    let _ = skyelab_git::status(&dest);
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "",
        "a filter only the new worktree's config defines ran"
    );
}
