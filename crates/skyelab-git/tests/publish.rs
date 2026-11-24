//! Acceptance: commit, push and open a PR under one account's `gh` config dir.
//! Pushes go to a local bare repo and `gh` is a fake. Nothing here reaches the network.
#![cfg(unix)] // the fake gh is a shell script

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use skyelab_git::{Error, GitHub};
use tempfile::TempDir;

fn fake_gh() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh")
}

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

/// A repo on branch `feature` (one commit on `main` below it) whose `origin` is a local bare repo,
/// plus an empty gh config dir.
fn setup() -> (TempDir, PathBuf, PathBuf, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let remote = base.join("remote.git");
    let repo = base.join("repo");
    let gh_dir = base.join("gh");
    fs::create_dir_all(&repo).expect("mkdir");
    fs::create_dir_all(&gh_dir).expect("mkdir");
    git(&base, &["init", "-q", "--bare", "remote.git"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    for kv in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&repo, &["config", kv.0, kv.1]);
    }
    fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(
        &repo,
        &["remote", "add", "origin", remote.to_str().expect("utf-8")],
    );
    git(&repo, &["checkout", "-q", "-b", "feature"]);
    (tmp, repo, remote, gh_dir)
}

fn gh(dir: &Path) -> GitHub<'_> {
    // Leaked so the borrow outlives the helper; a test-only path.
    let bin: &'static Path = Box::leak(fake_gh().into_boxed_path());
    GitHub {
        gh: bin,
        config_dir: dir,
    }
}

#[test]
fn commit_all_includes_untracked_and_reads_message_from_stdin() {
    let (_tmp, repo, _remote, _gh) = setup();
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    fs::write(repo.join("new.txt"), "new\n").expect("write");
    let sha = skyelab_git::commit(&repo, "-n looks like a flag\n\nbody", &[], &[], true, None)
        .expect("commit");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), sha);
    assert_eq!(
        git(&repo, &["log", "-1", "--format=%s"]),
        "-n looks like a flag"
    );
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}

#[test]
fn commit_selected_paths_only() {
    let (_tmp, repo, _remote, _gh) = setup();
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    fs::write(repo.join("-x"), "dash\n").expect("write");
    fs::write(repo.join("keep.txt"), "later\n").expect("write");
    skyelab_git::commit(
        &repo,
        "pick",
        &[PathBuf::from("a.txt"), PathBuf::from("-x")],
        &[],
        true,
        None,
    )
    .expect("commit");
    let files = git(&repo, &["show", "--name-only", "--format=", "HEAD"]);
    let mut files: Vec<_> = files.lines().collect();
    files.sort();
    assert_eq!(files, ["-x", "a.txt"]);
    assert_eq!(git(&repo, &["status", "--porcelain"]), "?? keep.txt");
}

#[test]
fn empty_message_is_refused_before_git_runs() {
    let (_tmp, repo, _remote, _gh) = setup();
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    let err = skyelab_git::commit(&repo, "  \n ", &[], &[], true, None).expect_err("empty");
    assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "M a.txt");
}

#[test]
fn push_sets_upstream_then_pushes_to_it() {
    let (_tmp, repo, remote, gh_dir) = setup();
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    skyelab_git::commit(&repo, "change", &[], &[], true, None).expect("commit");
    let to = skyelab_git::push(&repo, &gh(&gh_dir)).expect("push");
    assert_eq!(to, "origin/feature");
    assert_eq!(
        git(&remote, &["rev-parse", "refs/heads/feature"]),
        git(&repo, &["rev-parse", "HEAD"])
    );
    assert_eq!(
        git(&repo, &["rev-parse", "--abbrev-ref", "feature@{u}"]),
        "origin/feature"
    );

    fs::write(repo.join("a.txt"), "three\n").expect("write");
    skyelab_git::commit(&repo, "again", &[], &[], true, None).expect("commit");
    skyelab_git::push(&repo, &gh(&gh_dir)).expect("push again");
    assert_eq!(
        git(&remote, &["rev-parse", "refs/heads/feature"]),
        git(&repo, &["rev-parse", "HEAD"])
    );
}

#[test]
fn push_refuses_detached_head() {
    let (_tmp, repo, _remote, gh_dir) = setup();
    git(&repo, &["checkout", "-q", "--detach"]);
    assert!(skyelab_git::push(&repo, &gh(&gh_dir)).is_err());
}

/// The credential config push pins must ask the bound gh, never a helper from the user's global config.
#[test]
fn push_credentials_come_from_the_bound_gh() {
    let (tmp, repo, _remote, gh_dir) = setup();
    let marker = tmp.path().join("decoy-was-asked");
    let global = tmp.path().join("global.gitconfig");
    fs::write(
        &global,
        format!(
            "[credential]\n\thelper = \"!f() {{ touch '{m}'; }}; f\"\n\
             [credential \"https://github.com\"]\n\thelper = \"!f() {{ touch '{m}'; }}; f\"\n",
            m = marker.display()
        ),
    )
    .expect("write");
    let args = skyelab_git::credential_args(&gh(&gh_dir), &[]).expect("args");
    let mut child = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(&args)
        .args(["credential", "fill"])
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GH_CONFIG_DIR", &gh_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("git credential");
    std::io::Write::write_all(
        child.stdin.as_mut().expect("stdin"),
        b"protocol=https\nhost=github.com\n\n",
    )
    .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("password=fake-token"));
    assert!(!marker.exists(), "the global helper was asked");
    let asked = fs::read_to_string(gh_dir.join("fake-gh.credential")).expect("gh was asked");
    assert_eq!(
        asked.lines().collect::<Vec<_>>(),
        ["auth", "git-credential", "get"]
    );
}

#[test]
fn create_pr_passes_body_on_stdin_and_returns_the_url() {
    let (_tmp, repo, _remote, gh_dir) = setup();
    let body = "Fixes things.\n--draft is just text here\n";
    let url = skyelab_git::create_pr(&repo, &gh(&gh_dir), "-t Add feature", body).expect("pr");
    assert_eq!(url, "https://github.com/acme/demo/pull/1#feature");
    let args = fs::read_to_string(gh_dir.join("fake-gh.args")).expect("args");
    let args: Vec<_> = args.lines().collect();
    assert_eq!(
        args,
        [
            "pr",
            "create",
            "--head",
            "feature",
            "--title=-t Add feature",
            "--body-file",
            "-"
        ]
    );
    assert_eq!(
        fs::read_to_string(gh_dir.join("fake-gh.body")).expect("body"),
        body
    );
    let env = fs::read_to_string(gh_dir.join("fake-gh.env")).expect("env");
    assert!(
        env.lines()
            .any(|l| l == format!("GH_CONFIG_DIR={}", gh_dir.display())),
        "{env}"
    );
}

#[test]
fn create_pr_validates_before_gh_runs() {
    let (_tmp, repo, _remote, gh_dir) = setup();
    let gh = gh(&gh_dir);
    for title in ["", "  ", "two\nlines"] {
        let err = skyelab_git::create_pr(&repo, &gh, title, "b").expect_err(title);
        assert!(matches!(err, Error::Invalid(_)), "{err:?}");
    }
    let big = "x".repeat(64 * 1024 + 1);
    assert!(matches!(
        skyelab_git::create_pr(&repo, &gh, "t", &big),
        Err(Error::Invalid(_))
    ));
    assert!(!gh_dir.join("fake-gh.args").exists(), "gh ran");
}

#[test]
fn create_pr_without_a_url_in_the_output_is_an_error() {
    let (tmp, repo, _remote, gh_dir) = setup();
    let liar = tmp.path().join("liar-gh.sh");
    fs::write(&liar, "#!/bin/sh\ncat >/dev/null\necho 'all good'\n").expect("write");
    Command::new("chmod")
        .arg("755")
        .arg(&liar)
        .status()
        .expect("chmod");
    let gh = GitHub {
        gh: &liar,
        config_dir: &gh_dir,
    };
    assert!(skyelab_git::create_pr(&repo, &gh, "t", "b").is_err());
}

#[test]
fn gh_login_reads_the_active_github_login() {
    let (_tmp, _repo, _remote, gh_dir) = setup();
    assert_eq!(skyelab_git::gh_login(&gh(&gh_dir)).expect("status"), None);
    fs::write(gh_dir.join("fake-login"), "octo-acme").expect("write");
    assert_eq!(
        skyelab_git::gh_login(&gh(&gh_dir))
            .expect("status")
            .as_deref(),
        Some("octo-acme")
    );
}

const CHILD_MARKER: &str = "SKYELAB_GIT_PUBLISH_CHILD";

/// Run by `parent_tokens_never_reach_hooks_or_gh` in a copy of this binary whose env carries tokens.
#[test]
fn token_check_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    assert_eq!(std::env::var("GH_TOKEN").as_deref(), Ok("parent-gh-token"));
    let (tmp, repo, _remote, gh_dir) = setup();
    let hook_env = tmp.path().join("hook.env");
    let hook = repo.join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        format!("#!/bin/sh\nenv > '{}'\n", hook_env.display()),
    )
    .expect("hook");
    Command::new("chmod")
        .arg("755")
        .arg(&hook)
        .status()
        .expect("chmod");
    fs::write(repo.join("a.txt"), "two\n").expect("write");
    skyelab_git::commit(&repo, "c", &[], &[], true, None).expect("commit");
    skyelab_git::create_pr(&repo, &gh(&gh_dir), "t", "b").expect("pr");
    let leaked = |file: &Path| {
        let env = fs::read_to_string(file).expect("env file");
        env.lines()
            .filter(|l| {
                [
                    "GH_TOKEN=",
                    "GITHUB_TOKEN=",
                    "GH_REPO=",
                    "GH_HOST=",
                    "GH_ENTERPRISE_TOKEN=",
                ]
                .iter()
                .any(|p| l.starts_with(p))
                    || l.starts_with("GH_CONFIG_DIR=/wrong")
            })
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(leaked(&hook_env), Vec::<String>::new(), "hook env");
    assert_eq!(
        leaked(&gh_dir.join("fake-gh.env")),
        Vec::<String>::new(),
        "gh env"
    );
}

#[test]
fn parent_tokens_never_reach_hooks_or_gh() {
    let out = Command::new(std::env::current_exe().expect("test binary path"))
        .args(["token_check_in_child", "--exact", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .env("GH_ENTERPRISE_TOKEN", "parent-ent")
        .env("GH_REPO", "evil/repo")
        .env("GH_HOST", "evil.example")
        .env("GH_CONFIG_DIR", "/wrong/parent/gh")
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}
