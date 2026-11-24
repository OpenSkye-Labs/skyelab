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
    let cmd = command(Path::new("git"), Path::new(".")).expect("command");
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
    let cmd = command(Path::new("git"), Path::new(".")).expect("command");
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
    let cmd = remote::fetch_command(Path::new("."), "origin", &[], &gh).expect("command");
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

#[test]
fn repo_programs_are_switched_off_and_trusted_ones_kept() {
    let listing =
        b"global\0gpg.openpgp.program\n/opt/gpg\0global\0filter.lfs.clean\ngit-lfs clean -- %f\0\
local\0filter.lfs.clean\nevil\0local\0filter.ev.process\nevil\0worktree\0merge.ev.driver\nevil\0\
local\0gpg.program\nevil\0local\0core.sshcommand\nevil\0";
    let got = overrides_from(listing).expect("overrides");
    let get = |k: &str| {
        got.iter()
            .rev()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    };
    // Fixed names: the global value, else git's own default; never the repo's.
    assert_eq!(get("gpg.program"), Some("/opt/gpg"));
    assert_eq!(get("core.sshcommand"), Some("ssh"));
    assert_eq!(get("gpg.ssh.program"), Some("ssh-keygen"));
    assert_eq!(get("gpg.ssh.defaultkeycommand"), Some(""));
    // Drivers the repo defines: the global one survives, the repo's own goes empty.
    assert_eq!(get("filter.lfs.clean"), Some("git-lfs clean -- %f"));
    assert_eq!(get("filter.ev.process"), Some(""));
    assert_eq!(get("merge.ev.driver"), Some(""));
    for (k, v) in [
        ("core.hooksPath", crate::NO_HOOKS),
        ("core.fsmonitor", "false"),
        ("submodule.recurse", "false"),
        ("credential.helper", ""),
        ("core.askPass", ""),
    ] {
        assert_eq!(get(k), Some(v), "{k}");
    }
    assert!(!got.iter().any(|(_, v)| v == "evil"), "{got:?}");
    // A global-only driver is left to the global config.
    let global_only = overrides_from(b"global\0filter.lfs.smudge\ngit-lfs smudge\0").expect("ok");
    assert!(!global_only.iter().any(|(k, _)| k == "filter.lfs.smudge"));
}

#[test]
fn a_repo_driver_named_with_equals_or_non_utf8_is_refused() {
    assert!(overrides_from(b"local\0filter.a=b.clean\nx\0").is_err());
    assert!(overrides_from(b"local\0filter.\xff.clean\nx\0").is_err());
}

/// The snapshot's scratch index keeps the real index's mtime, so git's racy-entry check still re-reads a file
/// edited in the same timestamp tick as the index (Linux's `fs::copy` stamped the copy with now).
#[test]
fn the_scratch_index_keeps_the_index_mtime() {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("repo");
    std::fs::create_dir(&repo).expect("mkdir");
    git(&repo, &["init", "-q"]).expect("init");
    std::fs::write(repo.join("a.txt"), "one\n").expect("write");
    git(&repo, &["add", "a.txt"]).expect("add");
    let index = repo.join(".git").join("index");
    let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(&index)
        .expect("open index")
        .set_modified(old)
        .expect("age the index");
    let to = tmp.path().join("copy");
    commit::copy_index(&repo, &to).expect("copy");
    assert_eq!(
        std::fs::metadata(&to)
            .expect("copy")
            .modified()
            .expect("mtime"),
        old
    );
}

/// #13: nothing (another account's agent included) can make the folder git would look in for hooks with them off.
#[test]
fn no_hook_can_be_planted_under_the_hooks_off_path() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let dir = tmp.path().join(crate::NO_HOOKS);
    assert!(std::fs::create_dir_all(&dir).is_err(), "{}", dir.display());
    assert!(std::fs::write(dir.join("pre-commit"), "x").is_err());
}

#[test]
fn git_versions_with_the_ref_store_dir() {
    for (text, ok) in [
        ("git version 2.54.0 (Apple Git-157)\n", true),
        ("git version 2.56.0.windows.1", true),
        ("git version 3.0.0", true),
        ("git version 2.53.1", false),
        ("git version 2.39.5 (Apple Git-154)", false),
        ("nonsense", false),
    ] {
        assert_eq!(frozen::version_at_least(text, (2, 54)), ok, "{text}");
    }
}

#[test]
fn program_keys_are_recognised() {
    for key in [
        "filter.lfs.clean",
        "filter.a.b.smudge",
        "filter.x.process",
        "merge.ev.driver",
        "hook.cfg.command",
        "core.sshcommand",
        "gpg.program",
        "gpg.ssh.program",
        "gpg.ssh.defaultkeycommand",
    ] {
        assert!(frozen::is_program_key(key.as_bytes()), "{key}");
    }
    for key in [
        "filter.lfs.required",
        "merge.ev.name",
        "core.editor",
        "user.name",
        "filter.clean",
    ] {
        assert!(!frozen::is_program_key(key.as_bytes()), "{key}");
    }
}

/// The frozen config git reads back holds exactly the listed entries: quotes, backslashes, newlines, dotted and
/// quoted subsections, valueless and empty values, and multi-valued keys in order.
#[test]
fn the_frozen_config_reads_back_as_listed() {
    let entries: Vec<(&[u8], Option<&[u8]>)> = vec![
        (b"core.bare", Some(b"false")),
        (
            b"remote.origin.fetch",
            Some(b"+refs/heads/*:refs/remotes/origin/*"),
        ),
        (b"remote.origin.fetch", Some(b"+refs/tags/*:refs/tags/*")),
        (b"branch.a.b\"c\\d.merge", Some(b"refs/heads/x")),
        (b"user.name", Some(b"He said \"hi\" \\ there\n\tend")),
        (b"core.trustctime", None),
        (b"core.askpass", Some(b"")),
        (b"i18n.commitencoding", Some(b"latin1 \xe9")),
    ];
    let tmp = tempfile::tempdir().expect("tmp");
    let file = tmp.path().join("config");
    std::fs::write(&file, frozen::config_text(entries.iter().copied())).expect("write");
    let out = std::process::Command::new("git")
        .args(["config", "--file"])
        .arg(&file)
        .args(["--list", "-z"])
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut want = Vec::new();
    for (key, value) in &entries {
        want.extend_from_slice(key);
        if let Some(v) = value {
            want.push(b'\n');
            want.extend_from_slice(v);
        }
        want.push(0);
    }
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&want)
    );
}
