//! A project whose origin is missing or not on github.com has no repo to list: `repo_slug` says which, never with
//! git's own stderr.

use skyelab_git::{Error, NoGitHub, repo_slug};
use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .status()
        .expect("git runs")
        .success();
    assert!(ok, "git {args:?}");
}

fn why(dir: &Path) -> NoGitHub {
    match repo_slug(dir) {
        Err(Error::NoGitHub(why)) => why,
        other => panic!("no GitHub repo expected, got {other:?}"),
    }
}

#[test]
fn each_missing_github_remote_names_its_reason() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let d = tmp.path();
    assert_eq!(why(d), NoGitHub::NotARepo);
    git(d, &["init", "-q"]);
    assert_eq!(why(d), NoGitHub::NoOrigin);
    git(
        d,
        &["remote", "add", "upstream", "https://github.com/a/b.git"],
    );
    assert_eq!(why(d), NoGitHub::NoOrigin, "only origin is read");
    git(
        d,
        &["remote", "add", "origin", "https://gitlab.com/a/b.git"],
    );
    assert_eq!(why(d), NoGitHub::OtherHost);
    let shown = Error::NoGitHub(NoGitHub::OtherHost).to_string();
    assert!(
        !shown.contains("gitlab") && !shown.contains("git remote"),
        "{shown}"
    );
    git(
        d,
        &["remote", "set-url", "origin", "git@github.com:a/b.git"],
    );
    assert_eq!(repo_slug(d).expect("a github origin"), "a/b");
}
