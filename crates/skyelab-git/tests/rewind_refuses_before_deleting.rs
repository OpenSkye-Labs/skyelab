//! A rewind that refuses one path changes nothing: the files it would delete are still there.
#![cfg(unix)]
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{capture_checkpoint, plan_rewind, rewind};

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

fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path())
        .expect("canon")
        .join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::create_dir(repo.join("dir")).expect("mkdir");
    std::fs::write(repo.join("dir/f.txt"), "in dir\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo)
}

#[test]
fn a_refused_rewind_deletes_nothing() {
    let (tmp, repo) = repo();
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&outside).expect("mkdir");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    std::fs::write(repo.join("new.txt"), "user work\n").expect("w");
    std::fs::remove_dir_all(repo.join("dir")).expect("rm");
    std::os::unix::fs::symlink(&outside, repo.join("dir")).expect("link");
    std::fs::write(repo.join(".git/info/exclude"), "/dir\n").expect("w");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    assert_eq!(plan.remove, [PathBuf::from("new.txt")]);
    assert!(rewind(&repo, &plan, true).is_err(), "refused");
    assert!(repo.join("new.txt").exists(), "REFUSED_REWIND_DELETED_FILE");
}
