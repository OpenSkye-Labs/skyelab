//! Regression tests for findings from a code review.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{ChangeKind, Error, Line};
use tempfile::TempDir;

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .expect("git runs")
        .status
        .success()
}

fn git(dir: &Path, args: &[&str]) {
    assert!(git_ok(dir, args), "git {args:?} failed");
}

fn repo(tmp: &TempDir) -> PathBuf {
    let root = tmp.path().join("repo");
    fs::create_dir(&root).expect("mkdir");
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("a.txt"), "a\n\nb\n").expect("write");
    git(&root, &["add", "."]);
    git(&root, &["commit", "-q", "-m", "init"]);
    root
}

fn branch_exists(root: &Path, b: &str) -> bool {
    git_ok(
        root,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{b}"),
        ],
    )
}

#[test]
fn f1_blank_context_survives_suppress_blank_empty() {
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    git(&root, &["config", "diff.suppressBlankEmpty", "true"]);
    fs::write(root.join("a.txt"), "a\n\nB\n").expect("write");
    let d = skyelab_git::diff(&root, "HEAD").expect("diff");
    assert_eq!(
        d[0].hunks[0].lines,
        vec![
            Line::Context("a".into()),
            Line::Context(String::new()),
            Line::Removed("b".into()),
            Line::Added("B".into()),
        ]
    );
}

#[test]
fn f2_branch_cannot_inject_options() {
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    git(&root, &["branch", "victim"]);
    for bad in ["-D", "--force", "a..b", "x/../../y", "@{-1}"] {
        let err =
            skyelab_git::create_worktree(&root, bad, "victim", Some(&tmp.path().join("wt")), false)
                .expect_err(bad);
        assert!(matches!(err, Error::InvalidBranch(_)), "{bad}: {err:?}");
    }
    assert!(branch_exists(&root, "victim"));
}

#[test]
fn f3_base_cannot_inject_options() {
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    let res = skyelab_git::create_worktree(&root, "nb", "-m", Some(&tmp.path().join("wt")), false);
    assert!(res.is_err(), "{res:?}");
    assert!(branch_exists(&root, "main"));
    assert!(!branch_exists(&root, "nb"));
}

#[test]
fn f4_revision_syntax_is_not_a_branch() {
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    for rev in ["main^0", "main~0", "main^{/init}", "main@{0}"] {
        let res =
            skyelab_git::create_worktree(&root, rev, "main", Some(&tmp.path().join("wt")), true);
        assert!(
            matches!(res, Err(Error::InvalidBranch(_))),
            "{rev}: {res:?}"
        );
    }
}

#[test]
fn f5_submodule_log_does_not_leak() {
    let tmp = TempDir::new().expect("tempdir");
    let sub = tmp.path().join("s");
    fs::create_dir(&sub).expect("mkdir");
    git(&sub, &["init", "-q", "-b", "main"]);
    git(&sub, &["commit", "-q", "--allow-empty", "-m", "s1"]);
    let root = repo(&tmp);
    git(
        &root,
        &["submodule", "add", "-q", sub.to_str().expect("utf8"), "sub"],
    );
    git(&root, &["commit", "-q", "-m", "add sub"]);
    git(
        &root.join("sub"),
        &["commit", "-q", "--allow-empty", "-m", "moved"],
    );
    git(&root, &["config", "diff.submodule", "log"]);
    fs::write(root.join("a.txt"), "a\n\nB\n").expect("write");

    let d = skyelab_git::diff(&root, "HEAD").expect("diff");
    let a = d
        .iter()
        .find(|f| f.new_path.as_deref() == Some(Path::new("a.txt")))
        .expect("a.txt");
    assert_eq!(a.hunks[0].lines.len(), 4, "{a:#?}");
    assert!(
        d.iter()
            .any(|f| f.new_path.as_deref() == Some(Path::new("sub"))),
        "{d:#?}"
    );
}

#[cfg(unix)]
#[test]
fn f7_status_and_diff_do_not_run_fsmonitor() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    let marker = tmp.path().join("pwned");
    let hook = tmp.path().join("fsm.sh");
    fs::write(
        &hook,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .expect("write");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &root,
        &["config", "core.fsmonitor", hook.to_str().expect("utf8")],
    );

    let _ = skyelab_git::status(&root);
    let _ = skyelab_git::diff(&root, "HEAD");
    assert!(!marker.exists(), "core.fsmonitor program ran");
}

#[test]
fn f8_worktree_side_rename_keeps_orig_path() {
    let tmp = TempDir::new().expect("tempdir");
    let root = repo(&tmp);
    fs::rename(root.join("a.txt"), root.join("b.txt")).expect("mv");
    git(&root, &["add", "-N", "b.txt"]);
    let st = skyelab_git::status(&root).expect("status");
    for c in st.unstaged.iter().filter(|c| c.kind == ChangeKind::Renamed) {
        assert_eq!(c.orig_path.as_deref(), Some(Path::new("a.txt")), "{st:?}");
    }
}
