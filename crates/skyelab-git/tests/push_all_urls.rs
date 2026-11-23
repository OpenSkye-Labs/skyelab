//! Acceptance: Push checks every push URL (git pushes to all of them), and clears auth headers scoped to them.
//! Only local bare repos and a fake gh; nothing reaches the network.
#![cfg(unix)] // the fake gh is a shell script

use skyelab_git::GitHub;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

/// A repo on `feature` whose origin pushes to a local bare repo first, then to `second`.
fn setup(second: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let (repo, remote, gh_dir) = (base.join("repo"), base.join("remote.git"), base.join("gh"));
    fs::create_dir_all(&repo).expect("mkdir");
    fs::create_dir_all(&gh_dir).expect("mkdir");
    git(&base, &["init", "-q", "--bare", "remote.git"]);
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
    let remote_s = remote.to_str().expect("utf-8");
    git(&repo, &["remote", "add", "origin", remote_s]);
    git(
        &repo,
        &["config", "--add", "remote.origin.pushurl", remote_s],
    );
    git(&repo, &["config", "--add", "remote.origin.pushurl", second]);
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    (tmp, repo, remote, gh_dir)
}

#[test]
fn a_second_push_url_over_ssh_or_with_a_token_is_refused() {
    for second in [
        // example.invalid, so no insteadOf in the user's config can turn one into a real host
        "git@example.invalid:acme/demo.git",
        "ssh://git@example.invalid/acme/demo.git",
        "https://x-access-token:ghp_secret@example.invalid/acme/demo.git",
    ] {
        let (_tmp, repo, remote, gh_dir) = setup(second);
        let gh = GitHub {
            gh: &fake_gh(),
            config_dir: &gh_dir,
        };
        let err = skyelab_git::push(&repo, &gh).expect_err(second);
        assert!(!err.to_string().contains("ghp_secret"), "{err}");
        assert_eq!(
            git(&remote, &["branch", "--list", "feature"]),
            "",
            "pushed to the first URL anyway ({second})"
        );
    }
}

#[test]
fn every_plain_push_url_is_accepted() {
    let tmp2 = tempfile::tempdir().expect("tempdir");
    let other = fs::canonicalize(tmp2.path())
        .expect("canonical")
        .join("other.git");
    git(
        other.parent().expect("parent"),
        &["init", "-q", "--bare", "other.git"],
    );
    let (_tmp, repo, remote, gh_dir) = setup(other.to_str().expect("utf-8"));
    let gh = GitHub {
        gh: &fake_gh(),
        config_dir: &gh_dir,
    };
    skyelab_git::push(&repo, &gh).expect("push");
    for r in [&remote, &other] {
        assert_eq!(git(r, &["branch", "--list", "feature"]), "feature");
    }
}

#[test]
fn auth_headers_scoped_to_a_push_url_are_cleared() {
    let (_tmp, repo, _remote, gh_dir) = setup("https://github.com/acme/demo.git");
    let url = "https://github.com/acme/demo.git";
    git(
        &repo,
        &[
            "config",
            "http.https://github.com/.extraHeader",
            "AUTHORIZATION: a",
        ],
    );
    git(
        &repo,
        &[
            "config",
            &format!("http.{url}.extraHeader"),
            "AUTHORIZATION: b",
        ],
    );
    let gh = GitHub {
        gh: &fake_gh(),
        config_dir: &gh_dir,
    };
    let args = skyelab_git::credential_args(&gh, &[url]).expect("args");
    let out = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(&args)
        .args(["config", "--get-urlmatch", "http.extraHeader", url])
        .output()
        .expect("git runs");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "",
        "a header survives"
    );
}
