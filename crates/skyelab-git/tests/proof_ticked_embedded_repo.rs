//! Ticking an untracked nested repo (`sub/` in status) commits it, and the hook check doesn't undo it.
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

fn init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, &["config", k, v]);
    }
    std::fs::write(dir.join("a.txt"), "a\n").expect("write");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

#[test]
fn ticking_an_untracked_nested_repo_is_not_undone() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let repo = std::fs::canonicalize(tmp.path()).expect("canonical");
    init(&repo);
    let sub = repo.join("sub");
    std::fs::create_dir(&sub).expect("mkdir");
    init(&sub);
    let r = skyelab_git::commit(&repo, "c", &[PathBuf::from("sub/")], &[], true, None);
    assert!(r.is_ok(), "SPURIOUS_UNDO: {r:?}");
}
