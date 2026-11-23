//! The file picker lists tracked files plus untracked ones that aren't ignored.

use std::fs;
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}");
}

#[test]
fn project_files_lists_tracked_and_untracked_not_ignored() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    fs::create_dir_all(root.join("src")).expect("mkdir");
    fs::write(root.join(".gitignore"), "secret.env\n").expect("write");
    fs::write(root.join("src/lib.rs"), "").expect("write");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("secret.env"), "x").expect("ignored");
    fs::write(root.join("src/new.rs"), "x").expect("untracked");

    assert_eq!(
        skyelab_git::project_files(root).expect("list"),
        [".gitignore", "src/lib.rs", "src/new.rs"]
    );
    assert_eq!(
        skyelab_git::project_files(&root.join("src")).expect("list"),
        ["lib.rs", "new.rs"]
    );
}
