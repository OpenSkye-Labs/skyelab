//! Acceptance: a Codex title call is an ephemeral `codex exec` under the thread's own account's CODEX_HOME.
#![cfg(unix)]

use skyelab_agent::{generate_title, title_prompt};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;

#[test]
fn codex_title_runs_under_the_bound_codex_home() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    std::fs::create_dir(&dir).expect("mkdir");
    let account = Account {
        id: "cx".into(),
        label: "Codex".into(),
        provider: Provider::Codex,
        config_dir: root.join("codex-home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "cx".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let title = generate_title(
        &bin,
        &config,
        &account,
        &dir,
        &title_prompt("add a parser", "Added it."),
    )
    .expect("title");
    assert_eq!(title, "Codex fake title");
    let log = std::fs::read_to_string(dir.join(".fake-title-call")).expect("call logged");
    assert!(
        log.contains(&format!("codex_home={}", account.config_dir.display())),
        "{log}"
    );
    for flag in [
        "arg=[exec]",
        "arg=[--ephemeral]",
        "arg=[-s]\narg=[read-only]",
    ] {
        assert!(log.contains(flag), "missing {flag:?} in {log}");
    }
    assert!(log.contains("add a parser"));
}
