//! Acceptance: what a generated commit message or PR is written from. Only the thread's folder, untracked files only
//! when picked, never ignored files, the diff capped with a note; the PR side finds the base, the commits and the
//! repo's PR template.

use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{MAX_DIFF, TRUNCATED, commit_diff, pr_context};

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

/// A repo on `main` with `app/` (the project folder) and `other/` beside it, one commit.
fn repo() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    for d in ["app", "other"] {
        std::fs::create_dir(repo.join(d)).expect("mkdir");
        std::fs::write(repo.join(d).join("lib.txt"), "one\n").expect("w");
    }
    std::fs::write(repo.join(".gitignore"), "*.secret\n").expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    (tmp, repo)
}

#[test]
fn commit_diff_holds_only_the_folder_and_the_picked_files() {
    let (_tmp, repo) = repo();
    let app = repo.join("app");
    std::fs::write(app.join("lib.txt"), "one\nchanged in app\n").expect("w");
    std::fs::write(repo.join("other/lib.txt"), "changed outside\n").expect("w");
    std::fs::write(app.join("new.txt"), "brand new\n").expect("w");
    std::fs::write(app.join("notes.txt"), "not picked\n").expect("w");
    std::fs::write(app.join("key.secret"), "IGNORED-SECRET\n").expect("w");

    let all = commit_diff(&app, &[]).expect("diff");
    assert!(all.contains("+changed in app"), "{all}");
    assert!(
        all.contains("+brand new"),
        "untracked, nothing picked: {all}"
    );
    assert!(
        !all.contains("changed outside"),
        "outside the folder: {all}"
    );
    assert!(!all.contains("IGNORED-SECRET"), "ignored: {all}");

    let picked = commit_diff(
        &app,
        &[
            PathBuf::from("app/lib.txt"),
            PathBuf::from("app/new.txt"),
            PathBuf::from("other/lib.txt"),
        ],
    )
    .expect("diff");
    assert!(
        picked.contains("+changed in app") && picked.contains("+brand new"),
        "{picked}"
    );
    assert!(
        !picked.contains("not picked"),
        "an unpicked untracked file: {picked}"
    );
    assert!(
        !picked.contains("changed outside"),
        "a picked file outside the folder: {picked}"
    );
}

#[test]
fn a_big_diff_is_capped_with_a_note() {
    let (_tmp, repo) = repo();
    let line = "x".repeat(99);
    let big: String = (0..2_000).map(|i| format!("{i} {line}\n")).collect();
    std::fs::write(repo.join("app/lib.txt"), &big).expect("w");
    std::fs::write(repo.join("app/big-new.txt"), &big).expect("w");
    let d = commit_diff(&repo.join("app"), &[]).expect("diff");
    assert!(d.len() <= MAX_DIFF + TRUNCATED.len() + 1, "{}", d.len());
    assert!(d.trim_end().ends_with(TRUNCATED), "{}", &d[d.len() - 200..]);
}

#[test]
fn a_repo_with_no_commits_diffs_against_nothing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = std::fs::canonicalize(tmp.path()).expect("canon");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("first.txt"), "hello\n").expect("w");
    let d = commit_diff(&repo, &[]).expect("diff");
    assert!(d.contains("+hello"), "{d}");
}

#[test]
fn pr_context_has_the_commits_diff_and_template() {
    let (_tmp, repo) = repo();
    std::fs::create_dir(repo.join(".github")).expect("mkdir");
    std::fs::write(
        repo.join(".github/pull_request_template.md"),
        "## Summary\n\n## Testing\n",
    )
    .expect("w");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "add template"]);
    git(&repo, &["switch", "-q", "-c", "feature"]);
    std::fs::write(repo.join("app/lib.txt"), "one\nfeature work\n").expect("w");
    std::fs::write(repo.join("other/lib.txt"), "outside work\n").expect("w");
    git(&repo, &["commit", "-q", "-am", "feat: add feature work"]);

    let pr = pr_context(&repo.join("app")).expect("context");
    assert_eq!(pr.base, "main");
    assert_eq!(pr.commits.trim(), "- feat: add feature work");
    assert!(pr.diff.contains("+feature work"), "{}", pr.diff);
    assert!(!pr.diff.contains("outside work"), "{}", pr.diff);
    assert_eq!(pr.template.as_deref(), Some("## Summary\n\n## Testing\n"));
}

#[cfg(unix)]
#[test]
fn a_template_that_is_a_symlink_is_not_read() {
    let (_tmp, repo) = repo();
    std::fs::create_dir(repo.join(".github")).expect("mkdir");
    std::os::unix::fs::symlink("/etc/hosts", repo.join(".github/pull_request_template.md"))
        .expect("symlink");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "link"]);
    assert_eq!(pr_context(&repo).expect("context").template, None);
}
