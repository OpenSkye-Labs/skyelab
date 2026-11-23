//! Acceptance: the env the app builds for a bound terminal (the account's home, an empty per-account home for the
//! other provider, git's credential reset) reaches the child intact on both platforms, and the parent's personal
//! homes and tokens don't: unset on unix, blanked on Windows.

use super::launch;
use std::collections::HashMap;

const PARENT: &[&str] = &[
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GH_CONFIG_DIR",
    "GH_TOKEN",
    "ANTHROPIC_API_KEY",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_VALUE_0",
    "PATH",
];

fn account_env() -> HashMap<String, String> {
    [
        ("CLAUDE_CONFIG_DIR", "/acct/claude"),
        ("CODEX_HOME", "/data/skyelab/unbound/account-61636d65/codex"),
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "credential.helper"),
        ("GIT_CONFIG_VALUE_0", ""), // the reset: an empty helper, which must stay set
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[test]
fn account_env_reaches_the_child_and_personal_homes_do_not_on_both_platforms() {
    let parent: Vec<String> = PARENT.iter().map(|s| s.to_string()).collect();
    for windows in [false, true] {
        let (program, args, env) =
            launch(Some(("sh".into(), vec![])), account_env(), &parent, windows);
        for (k, v) in account_env() {
            assert_eq!(env.get(&k), Some(&v), "{k} windows={windows}");
        }
        let gone = ["GH_CONFIG_DIR", "GH_TOKEN", "ANTHROPIC_API_KEY"];
        if windows {
            assert_eq!(program, "sh");
            for v in gone {
                assert_eq!(env.get(v).map(String::as_str), Some(""), "{v} not blanked");
            }
            assert!(
                !env.contains_key("PATH"),
                "PATH is the parent's own, never overridden"
            );
        } else {
            assert_eq!(program, "/usr/bin/env");
            let unset: Vec<&str> = args
                .chunks(2)
                .take_while(|p| p[0] == "-u")
                .map(|p| p[1].as_str())
                .collect();
            let mut want = gone.to_vec();
            want.sort();
            let mut got = unset.clone();
            got.sort();
            assert_eq!(got, want, "exactly the personal vars are unset");
            for v in gone {
                assert!(!env.contains_key(v), "{v}");
            }
        }
    }
}
