//! The composer's `@` menu lists only what git tracks, relative to the folder asked about.

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
fn tracked_files_skips_ignored_and_untracked() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    git(root, &["init", "-q", "-b", "main"]);
    fs::create_dir_all(root.join("src/deep")).expect("mkdir");
    fs::write(root.join(".gitignore"), "secret.env\n").expect("write");
    fs::write(root.join("src/lib.rs"), "").expect("write");
    fs::write(root.join("src/deep/a b.rs"), "").expect("write");
    fs::write(root.join("README.md"), "").expect("write");
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "init"]);
    fs::write(root.join("secret.env"), "x").expect("ignored");
    fs::write(root.join("scratch.txt"), "x").expect("untracked");

    let mut all = skyelab_git::tracked_files(root).expect("list");
    all.sort();
    assert_eq!(
        all,
        [".gitignore", "README.md", "src/deep/a b.rs", "src/lib.rs"]
    );
    // From a subfolder, paths are relative to it.
    let mut sub = skyelab_git::tracked_files(&root.join("src")).expect("list");
    sub.sort();
    assert_eq!(sub, ["deep/a b.rs", "lib.rs"]);

    assert_eq!(
        skyelab_git::current_branch(root)
            .expect("branch")
            .as_deref(),
        Some("main")
    );
    git(root, &["checkout", "-q", "--detach"]);
    assert_eq!(skyelab_git::current_branch(root).expect("branch"), None);
    assert!(skyelab_git::tracked_files(&tmp.path().join("nope")).is_err());
}
