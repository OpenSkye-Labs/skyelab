//! Acceptance: a terminal never inherits the parent's gh login or tokens; the account's dirs pass.

use super::*;

const TOKENS: [&str; 8] = [
    "CODEX_API_KEY",
    "CODEX_ACCESS_TOKEN",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST",
    "GH_REPO",
];

/// The vars the child won't inherit, and the env it gets, on Windows or unix.
fn unset(account: &[(&str, &str)], windows: bool) -> (Vec<String>, HashMap<String, String>) {
    let env = account
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let (program, args, env) = launch(Some(("prog".into(), vec!["a".into()])), env, windows);
    if windows {
        assert_eq!(program, "prog");
        let gone = env
            .iter()
            .filter(|(_, v)| v.is_empty())
            .map(|(k, _)| k.clone())
            .collect();
        return (gone, env);
    }
    assert_eq!(program, "/usr/bin/env");
    let split = args
        .iter()
        .position(|a| a == "--")
        .expect("-- before the program");
    assert_eq!(&args[split + 1..], ["prog", "a"]);
    let gone = args[..split]
        .chunks(2)
        .map(|pair| {
            assert_eq!(pair[0], "-u");
            pair[1].clone()
        })
        .collect();
    (gone, env)
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn names(extra: &[&str]) -> Vec<String> {
    sorted(TOKENS.iter().chain(extra).map(|s| s.to_string()).collect())
}

#[test]
fn unbound_terminal_drops_every_account_var() {
    for windows in [false, true] {
        let (gone, _) = unset(&[], windows);
        assert_eq!(
            sorted(gone),
            names(&["CLAUDE_CONFIG_DIR", "CODEX_HOME", "GH_CONFIG_DIR"])
        );
    }
}

#[test]
fn bound_without_gh_dir_still_drops_gh_and_tokens() {
    for windows in [false, true] {
        let (gone, env) = unset(&[("CLAUDE_CONFIG_DIR", "/acct")], windows);
        assert_eq!(sorted(gone), names(&["CODEX_HOME", "GH_CONFIG_DIR"]));
        assert_eq!(env["CLAUDE_CONFIG_DIR"], "/acct");
    }
}

#[test]
fn bound_with_gh_dir_passes_both_dirs_and_drops_tokens() {
    for windows in [false, true] {
        let account = [("CLAUDE_CONFIG_DIR", "/acct"), ("GH_CONFIG_DIR", "/gh")];
        let (gone, env) = unset(&account, windows);
        assert_eq!(sorted(gone), names(&["CODEX_HOME"]));
        assert_eq!(env["CLAUDE_CONFIG_DIR"], "/acct");
        assert_eq!(env["GH_CONFIG_DIR"], "/gh");
    }
}
