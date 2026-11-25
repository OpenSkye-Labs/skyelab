//! Acceptance: hostile Cursor CLI stdout (oversized line, excessive blocks per message, event flood, invalid JSON)
//! stays strictly bounded in memory and emitted events.
#![cfg(unix)]

use skyelab_agent::{Event, Launch, Mode, start_with_bin};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-cursor.sh")
}

fn run_prompt(prompt: &str) -> Vec<Event> {
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
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt,
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Mode::Default,
    };
    let session = start_with_bin(&fake(), &config, &launch).expect("start");
    let mut events = Vec::new();
    while let Ok(ev) = session.events().recv_timeout(Duration::from_secs(60)) {
        if matches!(&ev, Event::Unknown(v) if v["type"] == "fake_env") {
            continue;
        }
        let done = matches!(ev, Event::Result { .. });
        events.push(ev);
        if done {
            break;
        }
    }
    events
}

#[test]
fn oversized_line_is_truncated_and_stream_recovers() {
    let events = run_prompt("oversized");
    let unknown = events
        .iter()
        .find_map(|e| match e {
            Event::Unknown(serde_json::Value::String(s)) => Some(s.len()),
            _ => None,
        })
        .expect("truncated Unknown event");
    assert!(unknown <= (16 << 20), "line exceeded 16 MiB cap: {unknown}");
    assert!(
        matches!(
            events.last(),
            Some(Event::Result {
                is_error: false,
                result: Some(r),
                ..
            }) if r == "after oversized"
        ),
        "{events:?}"
    );
}

#[test]
fn assistant_message_blocks_are_capped_per_line() {
    let events = run_prompt("many_blocks");
    let blocks = events
        .iter()
        .find_map(|e| match e {
            Event::Assistant { content } => Some(content.len()),
            _ => None,
        })
        .expect("assistant message");
    assert!(blocks <= 64, "expected <= 64 blocks, got {blocks}");
    assert!(matches!(
        events.last(),
        Some(Event::Result {
            is_error: false,
            ..
        })
    ));
}

#[test]
fn event_flood_is_capped_and_terminates_with_error_result() {
    let events = run_prompt("flood");
    assert!(
        events.len() <= 100_010,
        "emitted {} events past cap",
        events.len()
    );
    assert!(
        matches!(
            events.last(),
            Some(Event::Result {
                is_error: true,
                result: Some(msg),
                ..
            }) if msg.to_ascii_lowercase().contains("cap")
        ),
        "expected cap error result, got {:?}",
        events.last()
    );
}

#[test]
fn invalid_json_becomes_unknown_and_recovers() {
    let events = run_prompt("bad_json");
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Unknown(serde_json::Value::String(s)) if s == "not json at all"
    )));
    assert!(matches!(
        events.last(),
        Some(Event::Result {
            is_error: false,
            result: Some(r),
            ..
        }) if r == "recovered"
    ));
}
