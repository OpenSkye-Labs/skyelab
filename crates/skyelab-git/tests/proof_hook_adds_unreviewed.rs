//! A pre-commit hook (agent-writable) can't slip a file the user never saw into the commit.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
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
fn a_pre_commit_hook_cannot_add_a_file_the_user_never_saw() {
    let tmp = tempfile::tempdir().expect("tempdir");
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
    let hook = repo.join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\necho SECRET=1 > leak.env\ngit add leak.env\n",
    )
    .expect("hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
    fs::write(repo.join("a.txt"), "two\n").expect("edit");
    let tree = skyelab_git::snapshot_tree(&repo).expect("snapshot");
    let _ = skyelab_git::commit(&repo, "c", &[PathBuf::from("a.txt")], &[], Some(&tree));
    let files = git(&repo, &["ls-tree", "-r", "--name-only", "HEAD"]);
    assert!(
        !files.lines().any(|f| f == "leak.env"),
        "UNREVIEWED_FILE_COMMITTED: {files}"
    );
}
