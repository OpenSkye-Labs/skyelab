//! Acceptance: hostile ACP streams (oversized lines, giant tool payloads, flooded permission asks,
//! huge plans, turn-text flood, and hostile probe streams) stay within hard bounds without panicking or OOMing.
#![cfg(unix)]

use skyelab_agent::{
    Block, Decision, Event, Launch, Mode, TODO_TOOL, list_antigravity_models, login_status,
    start_with_bin, todos,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fake_antigravity() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-antigravity.sh")
}

fn rig() -> (tempfile::TempDir, PathBuf, PathBuf, Account, AccountsConfig) {
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
    (tmp, dir, gemini_home, account, config)
}

fn run_turn(prompt: &str) -> Vec<Event> {
    let (_tmp, dir, _home, account, config) = rig();
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
        untrusted: false,
    };
    let session = start_with_bin(&fake_antigravity(), &config, &launch).expect("start");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut out = Vec::new();
    while let Ok(ev) = session
        .events()
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        let done = matches!(ev, Event::Result { .. });
        out.push(ev);
        if done {
            break;
        }
    }
    out
}

#[test]
fn hostile_live_session_streams_are_bounded() {
    // 1. Flooded permission asks (> 64): only 64 open asks emitted, 6 excess immediately cancelled, 64 withdrawn at EOF.
    let (_tmp, dir, _home, account, config) = rig();
    let launch = Launch {
        account: &account,
        dir: &dir,
        resume: None,
        prompt: "flood_asks",
        mcp_config: None,
        model: None,
        effort: None,
        attachments: &[],
        mode: Mode::Default,
        untrusted: false,
    };
    let session = start_with_bin(&fake_antigravity(), &config, &launch).expect("start");
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut flood_events = Vec::new();
    while let Ok(ev) = session
        .events()
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        flood_events.push(ev);
    }
    let perms = flood_events
        .iter()
        .filter(|e| matches!(e, Event::Permission { .. }))
        .count();
    let withdrawn = flood_events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::PermissionResolved {
                    decision: Decision::Withdrawn,
                    ..
                }
            )
        })
        .count();
    let cancelled_replies = flood_events
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Unknown(v)
                    if v["method"] == "fake/request"
                        && v["params"]["result"]["outcome"]["outcome"] == "cancelled"
            )
        })
        .count();
    assert_eq!(perms, 64, "open asks capped at 64");
    assert_eq!(withdrawn, 64, "all 64 open asks withdrawn on EOF");
    assert_eq!(cancelled_replies, 6, "excess asks immediately cancelled");

    // 2. Huge plan (> 256 entries) capped at 256.
    let plan_events = run_turn("huge_plan");
    let plan_input = plan_events
        .iter()
        .find_map(|e| match e {
            Event::Assistant { content } => content.iter().find_map(|b| match b {
                Block::ToolUse { name, input, .. } if name == TODO_TOOL => Some(input),
                _ => None,
            }),
            _ => None,
        })
        .expect("plan TodoWrite");
    assert_eq!(todos(plan_input).len(), 256, "plan entries capped at 256");

    // 3. Giant tool payload sanitized and bounded.
    let giant_events = run_turn("giant_tool");
    let tool_input = giant_events
        .iter()
        .find_map(|e| match e {
            Event::Assistant { content } => content.iter().find_map(|b| match b {
                Block::ToolUse { id, input, .. } if id == "call_giant" => Some(input),
                _ => None,
            }),
            _ => None,
        })
        .expect("giant tool");
    assert!(
        tool_input.to_string().len() < 80_000,
        "giant tool input bounded"
    );

    // 4. Oversized line (> 16 MiB) truncated safely and stream recovers.
    let oversized_events = run_turn("oversized_line");
    assert!(oversized_events.iter().any(|e| matches!(
        e,
        Event::Assistant { content }
            if content.iter().any(|b| matches!(b, Block::Text { text } if text.contains("after oversized")))
    )));

    // 5. Turn text buffer (> 4 MiB) capped.
    let flood_text_events = run_turn("flood_text");
    let assistant_len: usize = flood_text_events
        .iter()
        .filter_map(|e| match e {
            Event::Assistant { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.len()),
            _ => None,
        })
        .sum();
    assert!(
        assistant_len <= (4 << 20) + 128,
        "turn text capped at ~4 MiB, got {assistant_len}"
    );
}

#[test]
fn hostile_probe_streams_fail_bounded() {
    // 1. Oversized probe line (> 4 MiB) rejected cleanly.
    let (_tmp, dir, home, account, config) = rig();
    std::fs::write(home.join("fake-probe-oversized"), "").expect("marker");
    assert!(login_status(&fake_antigravity(), &account).is_err());
    assert!(list_antigravity_models(&fake_antigravity(), &config, &account, &dir).is_err());

    // 2. Flooded probe frames (> 512 lines) rejected cleanly.
    std::fs::remove_file(home.join("fake-probe-oversized")).expect("rm");
    std::fs::write(home.join("fake-probe-flood"), "").expect("marker");
    assert!(login_status(&fake_antigravity(), &account).is_err());
    assert!(list_antigravity_models(&fake_antigravity(), &config, &account, &dir).is_err());
}
