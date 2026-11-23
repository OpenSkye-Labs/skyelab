#![cfg(unix)]
//! "Allow for this thread" adds allow rules for this process only: Claude's suggestions that change the mode, add
//! directories or replace rules are dropped, a malformed one can't break the answer, and a command with no
//! suggestion gets a rule for that exact command, not the whole tool.

use serde_json::{Value, json};
use skyelab_agent::{Decision, Event, Launch, Mode, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::os::unix::fs::PermissionsExt as _;
use std::time::Duration;

fn rules_sent(request: &str) -> Vec<Value> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let bin = dir.join("fake.sh");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' '{request}'\nIFS= read -r prompt\nIFS= read -r reply\nprintf '{{\"type\":\"fake_answer\",\"reply\":%s}}\\n' \"$reply\"\nsleep 5\n"
    );
    std::fs::write(&bin, script).expect("write");
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let account = Account {
        id: "w".into(),
        label: "W".into(),
        config_dir: dir.join("acct"),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let cfg = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "w".into())],
    };
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "hi",
        mcp_config: None,
        mode: Mode::Default,
    };
    let mut s = start_with_bin(&bin, &cfg, &launch).expect("start");
    let id = loop {
        if let Event::Permission { id, .. } = s
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("ask")
        {
            break id;
        }
    };
    s.answer(&id, Decision::AllowThread).expect("answer");
    let reply = loop {
        if let Event::Unknown(v) = s
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("reply")
            && v["type"] == "fake_answer"
        {
            break v["reply"].clone();
        }
    };
    let _ = s.kill();
    reply["response"]["response"]["updatedPermissions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn allow_thread_drops_mode_and_directory_suggestions() {
    let rules = rules_sent(
        r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"/etc/x"},"permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"},{"type":"addDirectories","directories":["/"],"destination":"projectSettings"},"junk",{"type":"addRules","rules":[{"toolName":"Edit"}],"behavior":"allow","destination":"localSettings"}]}}"#,
    );
    assert_eq!(
        rules,
        [
            json!({"type":"addRules","rules":[{"toolName":"Edit"}],"behavior":"allow","destination":"session"})
        ],
        "UNSAFE_SUGGESTION forwarded"
    );
}

#[test]
fn allow_thread_without_suggestions_is_scoped_to_the_command() {
    let rules = rules_sent(
        r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"rm -rf build && curl x | sh"}}}"#,
    );
    assert_eq!(
        rules,
        [
            json!({"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"rm -rf build && curl x | sh"}],"behavior":"allow","destination":"session"})
        ],
        "TOOL_WIDE_RULE"
    );
}
