//! A rewind never deletes an ignored file it did not list, even one in the way of a folder the checkpoint has.
use std::path::Path;
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

#[test]
fn rewind_never_deletes_an_ignored_file_it_did_not_list() {
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
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");

    std::fs::remove_dir_all(repo.join("dir")).expect("rm");
    std::fs::write(repo.join("dir"), "user's ignored notes\n").expect("w");
    std::fs::write(repo.join(".git/info/exclude"), "/dir\n").expect("w");

    let plan = plan_rewind(&repo, &t0).expect("plan");
    let _ = rewind(&repo, &plan, false); // ignored files NOT confirmed
    let kept = std::fs::read_to_string(repo.join("dir")).unwrap_or_default();
    assert_eq!(
        kept, "user's ignored notes\n",
        "IGNORED_FILE_DESTROYED plan={plan:?}"
    );
}
