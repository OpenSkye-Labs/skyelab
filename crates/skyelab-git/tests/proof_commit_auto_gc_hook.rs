//! Proof: a hooks-off commit never runs gc.recentObjectsHook from the repo's own config.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
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
fn commit_never_runs_the_repos_recent_objects_hook() {
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
    for (n, f) in ["a.txt", "b.txt"].iter().enumerate() {
        fs::write(repo.join(f), format!("{n}\n")).expect("write");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", f]);
        git(&repo, &["repack", "-q", "-d"]);
    }
    let marker = base.join("ran");
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");
    for (k, v) in [
        ("gc.autoPackLimit", "1"),
        ("gc.autoDetach", "false"),
        ("maintenance.autoDetach", "false"),
        ("gc.recentObjectsHook", evil.to_str().expect("utf-8")),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join("c.txt"), "c\n").expect("write");
    let none: &[PathBuf] = &[];
    let _ = skyelab_git::commit(&repo, "c", none, &[], false, None);
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "",
        "the repo's gc.recentObjectsHook ran"
    );
}
