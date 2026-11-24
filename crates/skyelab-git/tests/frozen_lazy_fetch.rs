//! A frozen read never runs a program the repo's config names for a lazy fetch: a promisor remote's upload-pack,
//! which git runs through sh when a diff needs an object a partial clone doesn't have.
#![cfg(unix)]

use std::fs;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str], input: Option<&str>) -> String {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git runs");
    if let Some(text) = input {
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(text.as_bytes())
            .expect("stdin");
    }
    let out = child.wait_with_output().expect("git waits");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn a_frozen_diff_never_runs_a_promisor_upload_pack() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"], None);
    git(&repo, &["config", "user.name", "t"], None);
    git(&repo, &["config", "user.email", "t@example.com"], None);
    fs::write(repo.join("a.txt"), "a\n").expect("write");
    git(&repo, &["add", "."], None);
    git(&repo, &["commit", "-q", "-m", "init"], None);
    // A commit whose tree names a blob the repo doesn't have.
    let missing = "1234567890123456789012345678901234567890";
    let tree = git(
        &repo,
        &["mktree", "--missing"],
        Some(&format!("100644 blob {missing}\ta.txt\n")),
    );
    let commit = git(
        &repo,
        &["commit-tree", &tree, "-p", "HEAD", "-m", "x"],
        None,
    );
    let marker = base.join("upload-pack-ran");
    let pack = base.join("evil-upload-pack");
    fs::write(
        &pack,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .expect("write");
    fs::set_permissions(&pack, fs::Permissions::from_mode(0o755)).expect("chmod");
    let nowhere = base.join("nowhere");
    for (k, v) in [
        ("core.repositoryformatversion", "1"),
        ("extensions.partialClone", "evil"),
        ("remote.evil.url", nowhere.to_str().expect("utf-8")),
        ("remote.evil.promisor", "true"),
        ("remote.evil.uploadpack", pack.to_str().expect("utf-8")),
    ] {
        git(&repo, &["config", k, v], None);
    }
    // Premise: plain git runs it for this diff.
    let _ = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["diff", "HEAD", &commit])
        .output()
        .expect("git");
    assert!(marker.exists(), "premise: plain git's lazy fetch runs it");
    fs::remove_file(&marker).expect("reset marker");
    let _ = skyelab_git::diff_trees(&repo, "HEAD", &commit);
    assert!(!marker.exists(), "the repo's upload-pack ran in a diff");
}
