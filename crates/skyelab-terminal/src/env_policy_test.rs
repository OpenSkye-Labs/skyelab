//! Acceptance: a terminal's child env is exactly the terminal-allowed parent vars, the caller's env, and
//! TERM/COLORTERM. Checked for both launch styles (unix `env -u`, Windows empty overrides) on every OS.

use super::launch;
use std::collections::{BTreeMap, HashMap};

const PARENT: &[(&str, &str)] = &[
    ("PATH", "/usr/bin:bin"),
    ("HOME", "/home/me"),
    ("EDITOR", "vim"),
    ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
    ("ANTHROPIC_API_KEY", "sk-personal"),
    ("ANTHROPIC_BASE_URL", "https://proxy.example"),
    ("CLAUDE_CODE_OAUTH_TOKEN", "personal"),
    ("CLAUDE_CONFIG_DIR", "/home/me/.claude-personal"),
    ("CODEX_HOME", "/home/me/.codex"),
    ("OPENAI_API_KEY", "sk-openai"),
    ("GH_TOKEN", "ghp_personal"),
    ("AWS_SECRET_ACCESS_KEY", "aws"),
    ("GIT_CONFIG_COUNT", "1"),
];

/// What the child ends up with: the parent minus what the launch removes, plus what it sets.
fn effective(windows: bool, caller: HashMap<String, String>) -> BTreeMap<String, String> {
    let names: Vec<String> = PARENT.iter().map(|(k, _)| k.to_string()).collect();
    let (program, args, env) = launch(Some(("prog".into(), vec![])), caller, &names, windows);
    let mut out: BTreeMap<String, String> = PARENT
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    if windows {
        assert_eq!(program, "prog");
    } else {
        assert_eq!(program, "/usr/bin/env");
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--" {
                break;
            }
            assert_eq!(a, "-u", "{args:?}");
            out.remove(it.next().expect("name after -u"));
        }
    }
    for (k, v) in env {
        if v.is_empty() {
            out.remove(&k); // Windows: an empty override is how a var is removed
        } else {
            out.insert(k, v);
        }
    }
    out
}

#[test]
fn child_env_is_the_allowlist_plus_the_caller_env() {
    for windows in [false, true] {
        let caller = HashMap::from([("CLAUDE_CONFIG_DIR".to_string(), "/acct".to_string())]);
        let got = effective(windows, caller);
        let mut want = BTreeMap::from([
            ("PATH".to_string(), "/usr/bin:bin".to_string()),
            ("EDITOR".into(), "vim".into()),
            ("SSH_AUTH_SOCK".into(), "/tmp/agent.sock".into()),
            ("CLAUDE_CONFIG_DIR".into(), "/acct".into()),
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
        ]);
        if !windows {
            want.insert("HOME".into(), "/home/me".into()); // a Windows profile is USERPROFILE, not HOME
        }
        assert_eq!(got, want, "windows={windows}");
    }
}
