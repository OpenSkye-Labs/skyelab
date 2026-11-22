#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use skyelab_git::GitHub;

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn push_never_authenticates_over_ssh() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let repo = base.join("repo");
    let gh_dir = base.join("gh");
    fs::create_dir_all(&repo).expect("mkdir");
    fs::create_dir_all(&gh_dir).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    let marker = base.join("ssh-ran");
    let ssh = base.join("fake-ssh.sh");
    fs::write(
        &ssh,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", marker.display()),
    )
    .expect("write");
    fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).expect("chmod");
    git(
        &repo,
        &["config", "core.sshCommand", ssh.to_str().expect("utf-8")],
    );
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "ssh://git@example.invalid/acme/demo.git",
        ],
    );
    let fake_gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let _ = skyelab_git::push(
        &repo,
        &GitHub {
            gh: &fake_gh,
            config_dir: &gh_dir,
        },
    );
    assert!(
        !marker.exists(),
        "push authenticated over ssh with a key no account binds"
    );
}
