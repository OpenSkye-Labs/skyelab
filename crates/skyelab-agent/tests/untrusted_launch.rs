//! Acceptance: a launch marked untrusted (a GitHub-started bot run) loads no setting source, so none of the account's
//! own allow rules can run a tool unasked; its deny and ask rules still reach the agent through `--settings`.
//! Nothing runs the real `claude`.
#![cfg(unix)]

use serde_json::{Value, json};
use skyelab_agent::{Event, Launch, Mode, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake_claude() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../skyelab-app/tests/fixtures/fake-claude.sh")
}

/// The fake's argv for one launch of `project/` under an account whose settings.json is `settings`.
fn argv(settings: Option<&str>, untrusted: bool) -> anyhow::Result<Vec<String>> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let dir = root.join("project");
    let config_dir = root.join("work-config");
    std::fs::create_dir(&dir).expect("mkdir");
    std::fs::create_dir(&config_dir).expect("mkdir");
    if let Some(s) = settings {
        std::fs::write(config_dir.join("settings.json"), s).expect("settings");
    }
    let account = Account {
        id: "work".into(),
        label: "work".into(),
        config_dir,
        provider: Provider::Claude,
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "work".into())],
    };
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Mode::Default,
        untrusted,
    };
    let session = start_with_bin(&fake_claude(), &config, &launch)?;
    loop {
        match session.events().recv_timeout(Duration::from_secs(60)) {
            Ok(Event::Unknown(v)) if v["type"] == "fake_env" => {
                return Ok(v["argv"]
                    .as_array()
                    .expect("argv")
                    .iter()
                    .map(|a| a.as_str().expect("a string").to_owned())
                    .collect());
            }
            Ok(_) => {}
            Err(e) => panic!("no fake_env line: {e}"),
        }
    }
}

fn after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let i = args.iter().position(|a| a == flag)?;
    args.get(i + 1).map(String::as_str)
}

const SETTINGS: &str = r#"{"permissions":{"allow":["Bash(make deploy)"],"deny":["Read(./.env)"],"ask":["Bash(git push:*)"]},"hooks":{}}"#;

#[test]
fn an_untrusted_run_loads_no_allow_rules_but_keeps_deny_and_ask() {
    let args = argv(Some(SETTINGS), true).expect("start");
    assert_eq!(after(&args, "--setting-sources"), Some(""), "{args:?}");
    assert_eq!(
        after(&args, "--permission-mode"),
        Some("default"),
        "{args:?}"
    );
    let settings: Value =
        serde_json::from_str(after(&args, "--settings").expect("--settings")).expect("JSON");
    assert_eq!(
        settings["permissions"],
        json!({ "deny": ["Read(./.env)"], "ask": ["Bash(git push:*)"] }),
        "{settings}"
    );
    assert!(!args.iter().any(|a| a.contains("make deploy")), "{args:?}");
    assert!(args.iter().any(|a| a == "--strict-mcp-config"), "{args:?}");
}

#[test]
fn a_trusted_run_still_loads_the_account_settings() {
    let args = argv(Some(SETTINGS), false).expect("start");
    assert_eq!(after(&args, "--setting-sources"), Some("user"), "{args:?}");
    assert!(after(&args, "--settings").is_none(), "{args:?}");
}

#[test]
fn an_untrusted_run_without_settings_starts_and_broken_settings_refuse() {
    let args = argv(None, true).expect("no settings.json is fine");
    assert_eq!(after(&args, "--setting-sources"), Some(""), "{args:?}");
    let err = argv(Some("{not json"), true).expect_err("refused");
    assert!(format!("{err:#}").contains("settings.json"), "{err:#}");
}
