//! The background fetch must not run a repo-set core.alternateRefsCommand: with alternates, git runs it through sh,
//! with the account's gh login in its env.
#![cfg(unix)]

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

fn identity(dir: &Path) {
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(dir, &["config", k, v]);
    }
}

#[test]
fn fetch_never_runs_a_repo_alternate_refs_command() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = std::fs::canonicalize(tmp.path()).expect("canonical");
    git(&base, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
    git(&base, &["init", "-q", "--bare", "-b", "main", "alt.git"]);
    git(&base, &["init", "-q", "-b", "main", "repo"]);
    let repo = base.join("repo");
    identity(&repo);
    std::fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let remote = base.join("remote.git");
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("utf-8")],
    );
    git(&repo, &["push", "-q", "-u", "origin", "main"]);
    git(
        &base,
        &["clone", "-q", remote.to_str().expect("utf-8"), "other"],
    );
    let other = base.join("other");
    identity(&other);
    std::fs::write(other.join("b.txt"), "two\n").expect("write");
    git(&other, &["add", "."]);
    git(&other, &["commit", "-q", "-m", "two"]);
    git(&other, &["push", "-q", "origin", "main"]);

    let marker = base.join("ran");
    std::fs::write(
        repo.join(".git/objects/info/alternates"),
        format!("{}\n", base.join("alt.git/objects").display()),
    )
    .expect("alternates");
    git(
        &repo,
        &[
            "config",
            "core.alternateRefsCommand",
            &format!("touch '{}'", marker.display()),
        ],
    );

    let gh_dir = base.join("gh");
    std::fs::create_dir_all(&gh_dir).expect("mkdir");
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let gh = GitHub {
        gh: &fake,
        config_dir: &gh_dir,
    };
    let r = skyelab_git::fetch(&repo, &gh);
    assert!(
        !marker.exists(),
        "core.alternateRefsCommand ran during the background fetch: {r:?}"
    );
    let err = r.expect_err("refused");
    assert!(
        err.to_string().contains("core.alternaterefscommand"),
        "{err}"
    );
}
