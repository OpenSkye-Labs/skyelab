//! Acceptance: a Cursor agent never inherits the parent's Cursor, Claude or Codex settings; it gets
//! exactly the bound account's `CURSOR_CONFIG_DIR`. Runs in a copy of this test binary with those vars planted.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::Path;
use std::time::Duration;

const CHILD_MARKER: &str = "SKYELAB_CURSOR_ENV_CHILD";
const PLANTED: [(&str, &str); 6] = [
    ("CURSOR_CONFIG_DIR", "/parent/cursor"),
    ("CURSOR_API_KEY", "cur-secret-parent"),
    ("CURSOR_AUTH_TOKEN", "cur-token-parent"),
    ("CURSOR_BACKEND_URL", "https://evil.example"),
    ("CLAUDE_CONFIG_DIR", "/parent/claude"),
    ("CODEX_HOME", "/parent/codex"),
];

#[test]
fn env_check_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    assert_eq!(
        std::env::var("CURSOR_API_KEY").as_deref(),
        Ok("cur-secret-parent")
    );
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let account = Account {
        id: "cur".into(),
        label: "Cursor".into(),
        provider: Provider::Cursor,
        config_dir: dir.join("cursor-home"),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let config = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "cur".into())],
    };
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-cursor.sh");
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
        untrusted: false,
    };
    let session = start_with_bin(&bin, &config, &launch).expect("start");
    let env = loop {
        match session
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event")
        {
            Event::Unknown(v) if v["type"] == "fake_env" => break v,
            _ => {}
        }
    };
    assert_eq!(
        env["cursor_config_dir"],
        dir.join("cursor-home").to_str().expect("utf-8")
    );
    for key in [
        "cursor_api_key",
        "cursor_auth_token",
        "cursor_backend_url",
        "claude_config_dir",
        "codex_home",
    ] {
        assert_eq!(env[key], "<unset>", "{key} leaked into cursor");
    }

    // Unbound or rebound folder starts nothing.
    let err = start_with_bin(&bin, &AccountsConfig::default(), &launch)
        .err()
        .expect("unbound folder must not start");
    assert!(err.to_string().contains("not bound"), "{err:#}");
    let mut moved = config.clone();
    moved.accounts[0].provider = Provider::Claude;
    let err = start_with_bin(&bin, &moved, &launch)
        .err()
        .expect("changed provider must not start");
    assert!(
        format!("{err:#}").contains("nothing was started"),
        "{err:#}"
    );
}

#[test]
fn cursor_gets_only_the_bound_cursor_config_dir() {
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
