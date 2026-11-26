//! A real claude wake result (T3's claude_background_monitor_wake transcript) carries `origin` and no
//! user_message_uuids. It answers none of our prompts, so it must not end the running turn.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

#[test]
fn a_wake_result_without_uuids_answers_no_prompt() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let bin = dir.join("fake-claude-wake.sh");
    std::fs::write(
        &bin,
        r#"#!/bin/sh
printf '{"type":"system","subtype":"init","cwd":"%s","session_id":"s","tools":[],"model":"fake","permissionMode":"default"}\n' "$(pwd -P)"
IFS= read -r first
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"duration_ms":1835,"num_turns":1,"result":"wake","session_id":"s","total_cost_usd":0,"origin":{"kind":"task-notification"},"queued_turn_count":0,"result_index":1}'
sleep 5
"#,
    )
    .expect("write");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let account = Account {
        id: "work".into(),
        label: "Work".into(),
        config_dir: dir.join(".claude-work"),
        provider: Default::default(),
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
        mode: Default::default(),
    };
    let mut session = start_with_bin(&bin, &config, &launch).expect("start");
    let prompts = loop {
        match session
            .events()
            .recv_timeout(Duration::from_secs(60))
            .expect("an event")
        {
            Event::Result { prompts, .. } => break prompts,
            _ => continue,
        }
    };
    session.kill().expect("kill");
    assert_eq!(
        prompts, 0,
        "WAKE_ENDED_A_TURN: a task-notification result answered {prompts} prompt(s)"
    );
}
