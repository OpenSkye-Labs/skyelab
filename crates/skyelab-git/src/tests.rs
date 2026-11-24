use std::path::Path;

use super::*;

#[test]
fn missing_git_binary_is_typed() {
    let err = git_with(
        Path::new("/definitely/not/a/git"),
        Path::new("."),
        &["--version"],
    )
    .expect_err("no binary");
    assert!(matches!(err, Error::GitNotInstalled), "{err:?}");
}

#[test]
fn unquotes_c_style_paths() {
    assert_eq!(
        diff::unquote(r#""tab\there \"q\" \\ \303\274""#),
        "tab\there \"q\" \\ ü"
    );
    assert_eq!(diff::unquote("plain name"), "plain name");
}

#[test]
fn parses_porcelain_worktree_list() {
    let out =
        b"worktree /r\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
worktree /r/wt with space\0HEAD 2222222222222222222222222222222222222222\0detached\0\0";
    let list = worktree::parse_list(out);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].branch.as_deref(), Some("main"));
    assert_eq!(list[1].path, Path::new("/r/wt with space"));
    assert!(list[1].detached);
    assert_eq!(list[1].branch, None);
}

/// Every var the command sets on top of `env_clear` is allowlisted (or the locale and the lock opt-out).
/// `get_envs` can't show `env_clear` itself; scrubbed_env_hooks.rs checks the real child env.
#[test]
fn command_env_is_only_the_allowlist() {
    let cmd = command(Path::new("git"), Path::new("."));
    for (k, _) in cmd.get_envs() {
        let ok = skyelab_core::env::agent_allowed(k, cfg!(windows), false)
            || k == "LC_ALL"
            || k == "GIT_OPTIONAL_LOCKS";
        assert!(ok, "{k:?} reaches git");
    }
}

/// None of the vars that point git at another repo is set on the command (`env_clear` drops inherited ones;
/// scrubbed_env_hooks.rs plants them in a real parent env).
#[test]
fn command_strips_inherited_repo_env() {
    let cmd = command(Path::new("git"), Path::new("."));
    let set: Vec<_> = cmd.get_envs().map(|(k, _)| k).collect();
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_CONFIG_PARAMETERS",
    ] {
        assert!(!set.contains(&std::ffi::OsStr::new(var)), "{var} is set");
    }
}

#[test]
fn default_dest_is_per_repo_and_stable() {
    // Pinned values: changing the hash would orphan every existing default worktree directory.
    assert_eq!(worktree::fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(worktree::fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    let Ok(a) = worktree::default_dest(Path::new("/one/repo"), "t/1") else {
        return;
    }; // no data dir on this host
    let b = worktree::default_dest(Path::new("/two/repo"), "t/1").expect("data dir");
    assert_ne!(a, b);
    assert!(a.ends_with("t/1"));
    assert!(
        a.starts_with(
            dirs::data_dir()
                .expect("data dir")
                .join(skyelab_core::instance::APP_DIR)
                .join("worktrees")
        )
    );
    let dir = a
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .expect("repo dir");
    assert!(dir.to_string_lossy().starts_with("repo-"), "{dir:?}");
}

#[test]
fn isolation_fetch_runs_with_the_account_gh_only() {
    let gh = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-gh.sh");
    let gh = GitHub {
        gh: &gh,
        config_dir: Path::new("/acct/gh"),
    };
    let cmd = remote::fetch_command(Path::new("/r"), "origin", &[], &gh).expect("command");
    let env: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    let get = |key: &str| {
        env.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.clone())
    };
    assert_eq!(get("GH_CONFIG_DIR").as_deref(), Some("/acct/gh"));
    assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    for leaked in [
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GIT_DIR",
    ] {
        assert!(get(leaked).is_none(), "{leaked} leaked: {env:?}");
    }
}
