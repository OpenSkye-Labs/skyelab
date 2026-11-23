//! Acceptance: a project terminal always names both provider homes (and, unbound, the GitHub dir): the bound
//! account's own, empty private "unbound" dirs for the rest, so `claude`, `codex` or `gh` typed there never fall
//! back to the default `~/.claude`, `~/.codex` or gh login. git's https credentials come only from the account's gh.

use super::terminal_env;
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn account(id: &str, provider: Provider, base: &Path, gh: bool) -> Account {
    Account {
        id: id.into(),
        label: id.into(),
        provider,
        config_dir: base.join(format!("{id}-home")),
        gh_config_dir: gh.then(|| base.join(format!("{id}-gh"))),
        ssh_agent: false,
    }
}

/// The `GIT_CONFIG_KEY_n`/`VALUE_n` pairs, in order, up to `GIT_CONFIG_COUNT`.
fn git_config(env: &HashMap<String, String>) -> Vec<(String, String)> {
    let n: usize = env
        .get("GIT_CONFIG_COUNT")
        .map(|c| c.parse().expect("count is a number"))
        .unwrap_or(0);
    (0..n)
        .map(|i| {
            (
                env[&format!("GIT_CONFIG_KEY_{i}")].clone(),
                env[&format!("GIT_CONFIG_VALUE_{i}")].clone(),
            )
        })
        .collect()
}

fn only_known_keys(env: &HashMap<String, String>) {
    for k in env.keys() {
        let known = [
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "GH_CONFIG_DIR",
            "GIT_CONFIG_COUNT",
        ]
        .contains(&k.as_str())
            || k.starts_with("GIT_CONFIG_KEY_")
            || k.starts_with("GIT_CONFIG_VALUE_");
        assert!(known, "unexpected {k} in {env:?}");
    }
}

#[cfg(unix)]
fn assert_private_empty(dir: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    let meta = std::fs::symlink_metadata(dir).expect("unbound dir exists");
    assert!(meta.is_dir(), "{dir}");
    assert_eq!(meta.permissions().mode() & 0o777, 0o700, "{dir}");
    assert_eq!(std::fs::read_dir(dir).expect("read").count(), 0, "{dir}");
}

#[cfg(not(unix))]
fn assert_private_empty(dir: &str) {
    assert!(Path::new(dir).is_dir(), "{dir}");
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf, AccountsConfig) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = std::fs::canonicalize(tmp.path()).expect("canonical");
    let (work, codex) = (base.join("work"), base.join("codex-proj"));
    std::fs::create_dir_all(&work).expect("mkdir");
    std::fs::create_dir_all(&codex).expect("mkdir");
    let config = AccountsConfig {
        accounts: vec![
            account("acme", Provider::Claude, &base, true),
            account("cx", Provider::Codex, &base, false),
        ],
        bindings: vec![
            Binding::new(work.clone(), "acme".into()),
            Binding::new(codex.clone(), "cx".into()),
        ],
    };
    (tmp, base, work, config)
}

#[test]
fn bound_claude_terminal_names_its_account_and_no_default_codex() {
    let (_tmp, base, work, config) = setup();
    let unbound = base.join("unbound");
    let env = terminal_env(&config, &work, &unbound).expect("env");
    only_known_keys(&env);
    let p = |p: PathBuf| p.to_str().expect("utf-8").to_string();
    assert_eq!(env["CLAUDE_CONFIG_DIR"], p(base.join("acme-home")));
    assert_eq!(env["GH_CONFIG_DIR"], p(base.join("acme-gh")));
    assert_eq!(env["CODEX_HOME"], p(unbound.join("account-61636d65/codex")));
    assert_private_empty(&env["CODEX_HOME"]);
    let git = git_config(&env);
    let has = |k: &str, v: &str| git.iter().any(|(a, b)| a == k && b == v);
    assert!(has("credential.helper", ""), "{git:?}");
    assert!(
        has("credential.helper", "!gh auth git-credential"),
        "{git:?}"
    );
    assert!(has("http.extraHeader", ""), "{git:?}");
    let empty = git
        .iter()
        .position(|(k, v)| k == "credential.helper" && v.is_empty());
    let gh = git.iter().position(|(_, v)| v == "!gh auth git-credential");
    assert!(
        empty < gh,
        "the reset must come before the gh helper: {git:?}"
    );
}

#[test]
fn bound_codex_terminal_mirrors_it() {
    let (_tmp, base, _work, config) = setup();
    let unbound = base.join("unbound");
    let env = terminal_env(&config, &base.join("codex-proj"), &unbound).expect("env");
    only_known_keys(&env);
    let p = |p: PathBuf| p.to_str().expect("utf-8").to_string();
    assert_eq!(env["CODEX_HOME"], p(base.join("cx-home")));
    assert_eq!(
        env["CLAUDE_CONFIG_DIR"],
        p(unbound.join("account-6378/claude"))
    );
    assert_private_empty(&env["CLAUDE_CONFIG_DIR"]);
    // No GitHub dir on the account: gh keeps its default, as for agents, and git gets no helper at all.
    assert!(!env.contains_key("GH_CONFIG_DIR"), "{env:?}");
    let git = git_config(&env);
    assert!(
        git.contains(&("credential.helper".into(), String::new())),
        "{git:?}"
    );
    assert!(!git.iter().any(|(_, v)| v.contains("gh auth")), "{git:?}");
}

#[test]
fn unbound_terminal_points_every_home_at_an_empty_private_dir() {
    let (_tmp, base, _work, config) = setup();
    let unbound = base.join("unbound");
    let env = terminal_env(&config, &base, &unbound).expect("env");
    only_known_keys(&env);
    for (var, sub) in [
        ("CLAUDE_CONFIG_DIR", "claude"),
        ("CODEX_HOME", "codex"),
        ("GH_CONFIG_DIR", "gh"),
    ] {
        assert_eq!(
            env[var],
            unbound.join(sub).to_str().expect("utf-8"),
            "{var}"
        );
        assert_private_empty(&env[var]);
    }
}
