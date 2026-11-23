//! Acceptance: ignoring whitespace drops whitespace-only changes and keeps real ones.

use super::*;
use std::process::Command;

fn git(dir: &std::path::Path, args: &[&str]) {
    let ok = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn ignore_whitespace_hides_whitespace_only_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let d = dir.path();
    git(d, &["init", "-q"]);
    std::fs::write(d.join("a.txt"), "one\ntwo\n").expect("write");
    std::fs::write(d.join("b.txt"), "x\n").expect("write");
    git(d, &["add", "."]);
    git(d, &["commit", "-qm", "init"]);
    std::fs::write(d.join("a.txt"), "one  \n\ttwo\n").expect("write");
    std::fs::write(d.join("b.txt"), "y\n").expect("write");

    let all = diff(d, "HEAD").expect("diff");
    assert!(all.iter().all(|f| !f.hunks.is_empty()), "{all:?}");

    let quiet = diff_with(d, Against::Rev("HEAD"), true).expect("diff -w");
    let hunks = |name: &str| {
        quiet
            .iter()
            .find(|f| f.new_path.as_deref() == Some(std::path::Path::new(name)))
            .map_or(0, |f| f.hunks.len())
    };
    assert_eq!(hunks("a.txt"), 0, "whitespace-only change shows no hunk");
    assert_eq!(hunks("b.txt"), 1, "a real change still shows");
}
