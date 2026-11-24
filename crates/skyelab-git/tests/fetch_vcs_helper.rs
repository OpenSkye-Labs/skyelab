//! Acceptance: the background fetch refuses a repo-set `remote.<name>.vcs`. With it, git hands even an https remote
//! to a `git-remote-<vcs>` program, which would run with the account's gh login in its env. An agent can write
//! `.git/config`, so it is the repo's own config that is refused.
#![cfg(unix)] // the fake gh is a shell script

use skyelab_git::GitHub;
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
fn isolation_fetch_refuses_a_repo_configured_vcs_helper() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = std::fs::canonicalize(tmp.path()).expect("canonical");
    git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    let repo = base.join("repo");
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", k, v]);
    }
    std::fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let remote = base.join("remote.git");
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("utf-8")],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    let gh_dir = base.join("gh");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let gh = GitHub {
        gh: &fake,
        config_dir: &gh_dir,
    };
    assert!(skyelab_git::fetch(&repo, &gh).expect("fetches before"));

    git(&repo, &["config", "remote.origin.vcs", "evil"]);
    let err = skyelab_git::fetch(&repo, &gh).expect_err("refused");
    assert!(err.to_string().contains("remote.origin.vcs"), "{err}");
    let pull = skyelab_git::pull_ff(&repo, &gh, &[], true).expect_err("pull fetches first");
    assert!(pull.to_string().contains("remote.origin.vcs"), "{pull}");
}
