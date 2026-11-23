//! The scratch index of a snapshot never writes through a symlink planted in `.git`.
#![cfg(unix)]
use std::fs;
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
fn snapshot_never_writes_through_a_planted_symlink() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    fs::create_dir_all(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    let victim = base.join("victim.txt");
    fs::write(&victim, "precious").expect("victim");
    let scratch = repo.join(format!(
        ".git/skyelab-snapshot-{}-0.index",
        std::process::id()
    ));
    std::os::unix::fs::symlink(&victim, &scratch).expect("plant");
    let _ = skyelab_git::snapshot_tree(&repo);
    assert_eq!(
        fs::read(&victim).expect("read victim"),
        b"precious",
        "SYMLINK_FOLLOWED"
    );
}
