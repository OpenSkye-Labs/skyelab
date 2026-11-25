//! Acceptance: one env allowlist for agents and terminals. Every credential, provider and base-URL var a parent may
//! carry is planted, and the exact result is checked for both name rules (unix, Windows) on every OS.

use skyelab_core::env::{agent_vars, terminal_vars};
use std::collections::BTreeSet;
use std::ffi::OsString;

/// Vars that pick an account, carry a login, or redirect a provider. None may pass, to agents or terminals.
const CREDENTIALS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_FOUNDRY_API_KEY",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CONFIG_DIR",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "CODEX_HOME",
    "CODEX_API_KEY",
    "CODEX_ACCESS_TOKEN",
    "CURSOR_CONFIG_DIR",
    "CURSOR_API_KEY",
    "CURSOR_AUTH_TOKEN",
    "CURSOR_BACKEND_URL",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_CONFIG_DIR",
    "GH_HOST",
    "GH_REPO",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AWS_PROFILE",
    "AWS_REGION",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "CLOUD_ML_REGION",
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_SSH_COMMAND",
];

const PLUMBING: &[&str] = &[
    "HOME",
    "USER",
    "SHELL",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
    "XDG_CONFIG_HOME",
    "HTTPS_PROXY",
    "https_proxy",
    "NO_PROXY",
    "NODE_EXTRA_CA_CERTS",
    "TZ",
];

const SHELL_ONLY: &[&str] = &["EDITOR", "VISUAL", "PAGER", "LESS", "COLORTERM"];

const WINDOWS: &[&str] = &["SystemRoot", "USERPROFILE", "APPDATA", "TEMP", "Path"];

fn parent() -> Vec<(OsString, OsString)> {
    CREDENTIALS
        .iter()
        .chain(PLUMBING)
        .chain(SHELL_ONLY)
        .chain(WINDOWS)
        .chain(&["SSH_AUTH_SOCK"])
        .map(|k| (OsString::from(k), OsString::from(value(k))))
        .collect()
}

/// Each planted var's value; PATH's is absolute, so an agent keeps it.
fn value(k: &str) -> String {
    match k {
        "Path" if cfg!(windows) => r"C:\bin".into(),
        "Path" => "/bin".into(),
        _ => format!("v-{k}"),
    }
}

fn names(vars: &[(OsString, OsString)]) -> BTreeSet<String> {
    vars.iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect()
}

fn set(items: &[&[&str]]) -> BTreeSet<String> {
    items
        .iter()
        .flat_map(|s| s.iter())
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn agents_get_exactly_the_plumbing_on_unix() {
    let got = names(&agent_vars(parent(), false, false));
    assert_eq!(got, set(&[PLUMBING]));
    let got = names(&agent_vars(parent(), false, true));
    assert_eq!(got, set(&[PLUMBING, &["SSH_AUTH_SOCK"]]));
}

#[test]
fn agents_get_exactly_the_plumbing_on_windows() {
    let expected = [
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "TZ",
    ];
    let got = names(&agent_vars(parent(), true, false));
    assert_eq!(got, set(&[&expected, WINDOWS]));
    let got = names(&agent_vars(parent(), true, true));
    assert_eq!(got, set(&[&expected, WINDOWS, &["SSH_AUTH_SOCK"]]));
}

#[test]
fn terminals_add_shell_vars_and_ssh_but_no_credentials() {
    let got = names(&terminal_vars(parent(), false));
    assert_eq!(got, set(&[PLUMBING, SHELL_ONLY, &["SSH_AUTH_SOCK"]]));
    let got = names(&terminal_vars(parent(), true));
    let expected = [
        "HTTPS_PROXY",
        "https_proxy",
        "NO_PROXY",
        "NODE_EXTRA_CA_CERTS",
        "TZ",
    ];
    assert_eq!(
        got,
        set(&[&expected, WINDOWS, SHELL_ONLY, &["SSH_AUTH_SOCK"]])
    );
}

/// An agent's PATH loses relative entries (they'd resolve inside the untrusted project); a terminal's is the user's.
#[cfg(unix)]
#[test]
fn path_keeps_relative_entries_only_for_terminals() {
    let parent = vec![(
        OsString::from("PATH"),
        OsString::from("/usr/bin:bin:.:/bin"),
    )];
    let agent = agent_vars(parent.clone(), false, false);
    assert_eq!(agent, vec![("PATH".into(), "/usr/bin:/bin".into())]);
    assert_eq!(terminal_vars(parent.clone(), false), parent);
}

/// The values pass unchanged, so a var is either the user's own or absent.
#[test]
fn values_pass_unchanged() {
    for (k, v) in terminal_vars(parent(), false) {
        assert_eq!(v, OsString::from(value(&k.to_string_lossy())));
    }
}
