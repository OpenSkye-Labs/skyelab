#![cfg(unix)] // the fake gh is a shell script
use std::fs;
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
fn a_branch_tracking_main_is_never_pushed_onto_main() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let remote = base.join("remote.git");
    let repo = base.join("repo");
    let gh_dir = base.join("gh");
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
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("utf-8")],
    );
    git(&repo, &["push", "-q", "origin", "main"]);
    git(
        &repo,
        &["checkout", "-q", "-b", "fix", "--track", "origin/main"],
    );
    let main_before = git(&remote, &["rev-parse", "refs/heads/main"]);
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    skyelab_git::commit(&repo, "fix", &[]).expect("commit");
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let _ = skyelab_git::push(
        &repo,
        &GitHub {
            gh: &bin,
            config_dir: &gh_dir,
        },
    );
    let main_after = git(&remote, &["rev-parse", "refs/heads/main"]);
    assert_eq!(main_after, main_before, "pushed onto main");
}
