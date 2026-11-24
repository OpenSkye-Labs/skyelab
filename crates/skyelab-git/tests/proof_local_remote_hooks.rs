//! Proof: a hooks-off Push to a local-path remote never runs that remote repo's own hooks.
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
fn hooks_off_push_never_runs_the_local_remotes_hooks() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let gh_dir = base.join("gh");
    fs::create_dir_all(&gh_dir).expect("gh dir");
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
    for name in [
        "pre-receive",
        "update",
        "post-receive",
        "reference-transaction",
    ] {
        let hook = base.join("remote.git/hooks").join(name);
        fs::write(
            &hook,
            format!("#!/bin/sh\necho {name} >> '{}'\n", marker.display()),
        )
        .expect("hook");
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod");
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
        "the remote repo's hooks ran with hooks off"
    );
}
