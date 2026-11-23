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

/// The env starts empty (`env_clear`, see scrubbed_env_hooks.rs for a real hostile parent), so every var the
/// command carries is one it set: the allowlist, the locale and the lock opt-out. Nothing that points git elsewhere.
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
                .join("skyelab/worktrees")
        )
    );
    let dir = a
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .expect("repo dir");
    assert!(dir.to_string_lossy().starts_with("repo-"), "{dir:?}");
}
