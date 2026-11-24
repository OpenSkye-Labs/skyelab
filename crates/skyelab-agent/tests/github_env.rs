//! Acceptance: an agent gets its account's `GH_CONFIG_DIR` and never the parent's gh login or tokens.
#![cfg(unix)]
mod common;

use common::start_in;
use skyelab_agent::Event;
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude-gh.sh")
}

fn gh_env(gh_config_dir: Option<&Path>) -> serde_json::Value {
    let name = if gh_config_dir.is_some() {
        "with"
    } else {
        "without"
    };
    let dir = std::env::temp_dir().join(format!("skyelab-agent-gh-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let dir = std::fs::canonicalize(&dir).expect("canonical");
    let cfg = AccountsConfig {
        accounts: vec![Account {
            id: "acme".into(),
            label: "Acme".into(),
            config_dir: dir.join("claude"),
            provider: Default::default(),
            gh_config_dir: gh_config_dir.map(Path::to_path_buf),
            ssh_agent: false,
        }],
        bindings: vec![Binding::new(dir.clone(), "acme".into())],
    };
    let session = start_in(&fake_bin(), &cfg, &dir, "hi").expect("start");
    loop {
        let event = session
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("event within 60 s");
        if let Event::Unknown(v) = event
            && v["type"] == "fake_gh_env"
        {
            return v;
        }
    }
}

const CHILD_MARKER: &str = "SKYELAB_AGENT_GH_CHILD";

/// Run by `bound_gh_dir_passes_parent_gh_env_does_not` with a hostile parent env.
#[test]
fn gh_env_in_child() {
    if std::env::var_os(CHILD_MARKER).is_none() {
        return;
    }
    let env = gh_env(Some(Path::new("/bound/gh")));
    assert_eq!(env["gh_config_dir"], "/bound/gh");
    assert_eq!(env["gh_token"], "", "parent GH_TOKEN leaked");
    assert_eq!(env["github_token"], "", "parent GITHUB_TOKEN leaked");

    let env = gh_env(None);
    assert_eq!(
        env["has_gh_config_dir"], "",
        "parent GH_CONFIG_DIR leaked: {env}"
    );
    assert_eq!(env["gh_token"], "");
}

#[test]
fn bound_gh_dir_passes_parent_gh_env_does_not() {
    let out = std::process::Command::new(std::env::current_exe().expect("test binary path"))
        .args(["gh_env_in_child", "--exact", "--nocapture"])
        .env(CHILD_MARKER, "1")
        .env("GH_CONFIG_DIR", "/wrong/parent/gh")
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .expect("run child test");
    let log =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{log}");
    assert!(log.contains("1 passed"), "{log}");
}
