//! Acceptance: git and gh that SkyeLab runs get the scrubbed env (the agents' allowlist), never the parent's
//! credentials. Read-only calls and worktree creation run no repo hooks; Commit runs them with the scrubbed env plus
//! the account env it is given. The hostile parent env lives in a child test process.
#![cfg(unix)] // hooks and the fake gh are shell scripts

use skyelab_git::GitHub;
use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;

const BASE: &str = "SKYELAB_GIT_SCRUB_BASE";
const PLANTED: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CONFIG_DIR",
    "OPENAI_API_KEY",
    "AWS_SECRET_ACCESS_KEY",
    "GH_TOKEN",
    "GITHUB_TOKEN",
];

/// Set inside hooks; inherited, any of them points git at another repo. Planted too, and the setup git here
/// drops them so only SkyeLab's own calls meet them.
const REPO_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_CONFIG_PARAMETERS",
];

fn git(dir: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    for var in REPO_VARS {
        cmd.env_remove(var);
    }
    let out = cmd
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

fn hook(repo: &Path, name: &str, body: &str) {
    let path = repo.join(".git/hooks").join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("hook");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn assert_clean_env(file: &Path) {
    let env = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    for var in PLANTED {
        let leaked = env
            .lines()
            .any(|l| l.split_once('=').is_some_and(|(k, _)| k == *var));
        assert!(!leaked, "{var} reached {}:\n{env}", file.display());
    }
    // git sets its own GIT_DIR for hooks; the parent's value must never be the one they see.
    assert!(
        !env.contains("=parent-"),
        "a parent value reached {}:\n{env}",
        file.display()
    );
}

fn setup(base: &Path) -> PathBuf {
    let repo = base.join("repo");
    fs::create_dir_all(&repo).expect("mkdir");
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
    // A clean filter dumps its env; an agent could write exactly this.
    let filter_env = base.join("filter.env");
    git(
        &repo,
        &[
            "config",
            "filter.dump.clean",
            &format!("sh -c 'env > \"{}\"; cat'", filter_env.display()),
        ],
    );
    fs::write(repo.join(".git/info/attributes"), "*.txt filter=dump\n").expect("attributes");
    for name in [
        "post-checkout",
        "reference-transaction",
        "post-index-change",
    ] {
        hook(
            &repo,
            name,
            &format!("touch \"{}\"", base.join(format!("{name}.ran")).display()),
        );
    }
    hook(
        &repo,
        "pre-commit",
        &format!("env > \"{}\"", base.join("pre-commit.env").display()),
    );
    repo
}

#[test]
fn scrubbed_in_child() {
    let Some(base) = std::env::var_os(BASE) else {
        return;
    };
    let base = PathBuf::from(base);
    let repo = setup(&base);
    fs::write(repo.join("a.txt"), "two\n").expect("modify");

    skyelab_git::status(&repo).expect("status");
    skyelab_git::diff(&repo, "HEAD").expect("diff");
    skyelab_git::create_worktree(&repo, "wt", "main", Some(&base.join("wt")), false)
        .expect("worktree");
    for name in [
        "post-checkout",
        "reference-transaction",
        "post-index-change",
    ] {
        assert!(!base.join(format!("{name}.ran")).exists(), "{name} ran");
    }
    assert_clean_env(&base.join("filter.env"));

    let account = [("CLAUDE_CONFIG_DIR".into(), "/acct/claude".into())];
    skyelab_git::commit(&repo, "change", &[], &account, None).expect("commit");
    let hook_env = base.join("pre-commit.env");
    let text = fs::read_to_string(&hook_env).expect("pre-commit ran");
    assert!(
        text.lines().any(|l| l == "CLAUDE_CONFIG_DIR=/acct/claude"),
        "{text}"
    );
    for var in PLANTED.iter().filter(|v| **v != "CLAUDE_CONFIG_DIR") {
        assert!(
            !text.lines().any(|l| l.starts_with(&format!("{var}="))),
            "{var}: {text}"
        );
    }
    assert!(
        !text.contains("=parent-"),
        "a parent value reached the hook: {text}"
    );

    let gh_dir = base.join("gh");
    fs::create_dir_all(&gh_dir).expect("mkdir");
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    skyelab_git::gh_login(&GitHub {
        gh: &gh,
        config_dir: &gh_dir,
    })
    .expect("gh login check");
    assert_clean_env(&gh_dir.join("fake-gh.fullenv"));
    fs::remove_file(gh_dir.join("fake-gh.fullenv")).expect("reset");
    let gh = GitHub {
        gh: &gh,
        config_dir: &gh_dir,
    };
    skyelab_git::create_pr(&repo, &gh, "t", "b").expect("pr create");
    assert_clean_env(&gh_dir.join("fake-gh.fullenv"));
}

#[test]
fn skyelab_git_never_hands_parent_credentials_to_hooks_filters_or_gh() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = fs::canonicalize(tmp.path()).expect("canonical");
    let mut cmd = Command::new(std::env::current_exe().expect("test binary path"));
    cmd.args(["scrubbed_in_child", "--exact", "--nocapture"])
        .env(BASE, &base);
    for var in PLANTED.iter().chain(REPO_VARS) {
        cmd.env(var, format!("parent-{var}"));
    }
    let out = cmd.output().expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}
