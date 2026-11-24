//! Proof: a hooks-off Push to a local-path remote never runs a program that remote's own config names.
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
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
fn push_never_runs_the_local_remotes_programs() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let gh_dir = base.join("gh");
    fs::create_dir_all(&gh_dir).expect("gh dir");
    git(&base, &["init", "-q", "--bare", "-b", "main", "other.git"]);
    git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    git(&base, &["clone", "-q", "remote.git", "repo"]);
    let repo = base.join("repo");
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join("a.txt"), "1\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let marker = base.join("ran");
    let evil = base.join("evil");
    fs::write(
        &evil,
        format!("#!/bin/sh\necho ran >> '{}'\n", marker.display()),
    )
    .expect("evil");
    fs::set_permissions(&evil, fs::Permissions::from_mode(0o755)).expect("chmod");
    let remote = base.join("remote.git");
    fs::write(
        remote.join("objects/info/alternates"),
        format!("{}\n", base.join("other.git/objects").display()),
    )
    .expect("alternates");
    for (k, v) in [
        ("core.alternateRefsCommand", evil.to_str().expect("utf-8")),
        ("core.fsmonitor", evil.to_str().expect("utf-8")),
        ("gc.recentObjectsHook", evil.to_str().expect("utf-8")),
        ("gc.auto", "1"),
        ("gc.autoDetach", "false"),
        ("receive.autogc", "true"),
    ] {
        git(&remote, &["config", k, v]);
    }
    let bin: &'static Path = Box::leak(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake-gh.sh")
            .into_boxed_path(),
    );
    let gh = skyelab_git::GitHub {
        gh: bin,
        config_dir: &gh_dir,
    };
    let _ = skyelab_git::push_with(&repo, &gh, &[], false);
    assert_eq!(
        fs::read_to_string(&marker).unwrap_or_default(),
        "",
        "the local remote's config ran a program"
    );
}
