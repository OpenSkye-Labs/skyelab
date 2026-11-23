#![cfg(unix)]
use std::fs;
use std::path::{Path, PathBuf};
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

#[test]
fn a_ticked_file_named_star_commits_only_itself() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let repo = fs::canonicalize(tmp.path()).expect("canonical");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    fs::write(repo.join("*"), "star\n").expect("write");
    fs::write(repo.join("secret.txt"), "not ticked\n").expect("write");
    skyelab_git::commit(&repo, "star only", &[PathBuf::from("*")], &[], None).expect("commit");
    let files = git(&repo, &["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, "*", "committed more than the ticked file: {files}");
}
