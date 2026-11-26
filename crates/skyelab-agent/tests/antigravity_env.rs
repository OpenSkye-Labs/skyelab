//! Acceptance: an Antigravity agent never inherits the parent's Gemini, Google, Claude or Codex credentials;
//! it receives only the bound account's `GEMINI_HOME` plus keyring-isolation env and `--gemini-dir`.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;
use std::time::Duration;

const CHILD_MARKER: &str = "SKYELAB_ANTIGRAVITY_ENV_CHILD";
const PLANTED: [(&str, &str); 6] = [
    ("GEMINI_HOME", "/parent/gemini"),
    ("GEMINI_API_KEY", "gem-parent-secret"),
    ("GOOGLE_API_KEY", "goog-parent-secret"),
    ("GOOGLE_APPLICATION_CREDENTIALS", "/parent/adc.json"),
    ("CLAUDE_CONFIG_DIR", "/parent/claude"),
    ("CODEX_HOME", "/parent/codex"),
];

#[test]
fn env_check_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    assert_eq!(
        std::env::var("GEMINI_API_KEY").as_deref(),
        Ok("gem-parent-secret")
    );
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let gemini_home = dir.join("gemini-home");
    std::fs::create_dir_all(&gemini_home).expect("mkdir");
    std::fs::write(gemini_home.join("fake-login"), "me@example.com\n").expect("login");
    let account = Account {
        id: "agy".into(),
        label: "Antigravity".into(),
        provider: Provider::Antigravity,
        config_dir: gemini_home.clone(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "agy".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-antigravity.sh");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Default::default(),
    };
    let session = start_with_bin(&bin, &config, &launch).expect("start");
    let env = loop {
        match session
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event")
        {
            Event::Unknown(v) if v["method"] == "fake/env" => break v["params"].clone(),
            _ => {}
        }
    };
    let expected_home = gemini_home.to_str().expect("utf-8");
    assert_eq!(env["gemini_home"], expected_home);
    assert_eq!(env["agy_force_file"], "1");
    assert_eq!(env["agy_disable_update"], "true");
    assert_ne!(
        env["ssh_connection"], "<unset>",
        "SSH_CONNECTION isolates OS keyring"
    );
    let args = env["args"].as_str().expect("args");
    assert!(
        args.contains("--gemini-dir") && args.contains(expected_home),
        "{args}"
    );
    assert!(args.contains("--app-data-dir antigravity-acp"), "{args}");
    for key in [
        "claude_config_dir",
        "codex_home",
        "gemini_api_key",
        "google_api_key",
        "google_application_credentials",
    ] {
        assert_eq!(env[key], "<unset>", "{key} leaked into antigravity");
    }
}

#[test]
fn antigravity_gets_only_the_bound_gemini_home() {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args(["env_check_in_child", "--exact", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .envs(PLANTED)
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}
