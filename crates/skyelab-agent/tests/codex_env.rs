//! Acceptance: a Codex agent never inherits the parent's Codex, Claude or OpenAI settings; it gets
//! exactly the bound account's `CODEX_HOME`. Runs in a copy of this test binary with those vars planted.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;
use std::time::Duration;

const CHILD_MARKER: &str = "SKYELAB_CODEX_ENV_CHILD";
const PLANTED: [(&str, &str); 7] = [
    ("CODEX_HOME", "/parent/codex"),
    ("CLAUDE_CONFIG_DIR", "/parent/claude"),
    ("OPENAI_API_KEY", "sk-parent"),
    ("OPENAI_BASE_URL", "https://evil.example"),
    ("OPENAI_ORG_ID", "org-parent"),
    ("CODEX_API_KEY", "codex-parent"),
    ("CODEX_ACCESS_TOKEN", "token-parent"),
];

#[test]
fn env_check_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    assert_eq!(std::env::var("OPENAI_API_KEY").as_deref(), Ok("sk-parent"));
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let account = Account {
        id: "oai".into(),
        label: "OpenAI".into(),
        provider: Provider::Codex,
        config_dir: dir.join("codex-home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "oai".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-codex.sh");
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
    };
    let session = start_with_bin(&bin, &config, &launch).expect("start");
    let env = loop {
        match session
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event")
        {
            Event::Unknown(v) if v["method"] == "fake/env" => break v["params"].clone(),
            _ => {}
        }
    };
    assert_eq!(
        env["codex_home"],
        dir.join("codex-home").to_str().expect("utf-8")
    );
    for key in [
        "claude_config_dir",
        "openai_api_key",
        "openai_base_url",
        "openai_org_id",
        "codex_api_key",
        "codex_access_token",
    ] {
        assert_eq!(env[key], "<unset>", "{key} leaked into codex");
    }
}

#[test]
fn codex_gets_only_the_bound_codex_home() {
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
