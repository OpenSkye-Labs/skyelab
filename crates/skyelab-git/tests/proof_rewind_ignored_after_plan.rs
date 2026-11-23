//! An ignored file that appears after the rewind was shown is not overwritten: the rewind refuses instead.
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

fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
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
    std::fs::write(repo.join("keep.txt"), "k\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo)
}

#[test]
fn an_ignored_file_made_after_the_plan_is_not_overwritten() {
    let (_tmp, repo) = repo();
    std::fs::write(repo.join("a.txt"), "old\n").expect("w");
    let t0 = capture_checkpoint(&repo, "k", 0).expect("turn 0");
    std::fs::remove_file(repo.join("a.txt")).expect("rm");
    std::fs::write(repo.join(".gitignore"), "/a.txt\n").expect("w");
    let plan = plan_rewind(&repo, &t0).expect("plan");
    assert!(
        plan.restore.iter().any(|p| p == Path::new("a.txt")),
        "{plan:?}"
    );
    std::fs::write(repo.join("a.txt"), "precious\n").expect("w");
    let _ = rewind(&repo, &plan, false);
    let kept = std::fs::read_to_string(repo.join("a.txt")).unwrap_or_default();
    assert_eq!(kept, "precious\n", "IGNORED_FILE_OVERWRITTEN plan={plan:?}");
}

#[test]
fn rewinding_a_subfolder_project_keeps_its_folder() {
    let (_tmp, repo) = repo();
    let app = repo.join("app");
    std::fs::create_dir(&app).expect("mkdir");
    let t0 = capture_checkpoint(&app, "k", 0).expect("turn 0");
    std::fs::write(app.join("new.txt"), "agent\n").expect("w");
    std::fs::write(repo.join("outside-app.txt"), "sibling\n").expect("w");
    let plan = plan_rewind(&app, &t0).expect("plan");
    assert!(
        plan.restore.is_empty() && plan.ignored.is_empty(),
        "{plan:?}"
    );
    assert_eq!(
        plan.remove,
        [Path::new("app/new.txt")],
        "only the folder's own files"
    );
    rewind(&app, &plan, false).expect("rewind");
    assert!(app.is_dir(), "PROJECT_FOLDER_DELETED plan={plan:?}");
    assert!(repo.join("outside-app.txt").exists());
}
