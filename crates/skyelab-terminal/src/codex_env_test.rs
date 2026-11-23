//! Acceptance: a terminal never inherits the parent's Codex home or Codex credentials; a Codex account's home passes.

use super::*;

const CODEX: [&str; 3] = ["CODEX_HOME", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"];

#[test]
fn every_codex_var_is_an_account_var() {
    for v in CODEX {
        for windows in [false, true] {
            for ssh in [false, true] {
                assert!(
                    !skyelab_core::env::agent_allowed(v.as_ref(), windows, ssh),
                    "{v}"
                );
            }
        }
    }
}

#[test]
fn no_codex_var_is_terminal_allowed() {
    for v in CODEX {
        for windows in [false, true] {
            assert!(
                !skyelab_core::env::terminal_allowed(v.as_ref(), windows),
                "{v}"
            );
        }
    }
}

#[test]
fn codex_home_passes_only_when_the_account_sets_it() {
    let removed = |env: &[(&str, &str)]| {
        let env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let parent = CODEX.map(String::from);
        let (_, args, env) = launch(Some(("prog".into(), vec![])), env, &parent, false);
        (args, env)
    };
    let (args, _) = removed(&[]);
    for v in CODEX {
        assert!(
            args.windows(2).any(|w| w[0] == "-u" && w[1] == v),
            "{v} in {args:?}"
        );
    }
    let (args, env) = removed(&[("CODEX_HOME", "/acct/codex")]);
    assert!(!args.iter().any(|a| a == "CODEX_HOME"), "{args:?}");
    assert_eq!(env["CODEX_HOME"], "/acct/codex");
}
