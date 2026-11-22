//! Acceptance: a terminal never inherits the parent's Codex home or Codex credentials; a Codex account's home passes.

use super::*;

const CODEX: [&str; 3] = ["CODEX_HOME", "CODEX_API_KEY", "CODEX_ACCESS_TOKEN"];

#[test]
fn every_codex_var_is_an_account_var() {
    for v in CODEX {
        assert!(ACCOUNT_VARS.contains(&v), "{v}");
    }
}

#[cfg(unix)]
#[test]
fn codex_home_passes_only_when_the_account_sets_it() {
    let removed = |env: &[(&str, &str)]| {
        let env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let (_, args, env) = launch(Some(("prog".into(), vec![])), env);
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
