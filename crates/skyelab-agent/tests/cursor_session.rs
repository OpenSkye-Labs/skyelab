//! Acceptance: a Cursor-bound folder runs `cursor-agent` (or `agent` fallback; here the fake) under its
//! bound `CURSOR_CONFIG_DIR`, streams thinking/text/tool calls/results onto `Event`, supports `--resume`,
//! permission modes, text attachments (refusing binary ones), and interrupts cleanly.
#![cfg(unix)]

use serde_json::Value;
use skyelab_agent::{
    Attachment, Block, DeltaKind, Event, Launch, Mode, Session, Usage, start_with_bin,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-cursor.sh")
}

struct Fixture {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    account: Account,
    config: AccountsConfig,
}

fn bound() -> Fixture {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path())
        .expect("canonical")
        .join("project");
    std::fs::create_dir(&dir).expect("mkdir");
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
    Fixture {
        _tmp: tmp,
        dir,
        account,
        config,
    }
}

fn start_with(
    f: &Fixture,
    resume: Option<&str>,
    prompt: &str,
    model: Option<&str>,
    mode: Mode,
    attachments: &[Attachment],
) -> anyhow::Result<Session> {
    let launch = Launch {
        account: &f.account,
        dir: &f.dir,
        resume,
        prompt,
        mcp_config: None,
        model,
        effort: None,
        attachments,
        mode,
    };
    start_with_bin(&fake(), &f.config, &launch)
}

fn next(rx: &flume::Receiver<Event>) -> Event {
    rx.recv_timeout(Duration::from_secs(60))
        .expect("event within 60 s")
}

fn turn(rx: &flume::Receiver<Event>, env: &mut Option<Value>) -> Vec<Event> {
    let mut out = Vec::new();
    loop {
        match next(rx) {
            Event::Unknown(v) if v["type"] == "fake_env" => *env = Some(v),
            e @ Event::Result { .. } => {
                out.push(e);
                return out;
            }
            e => out.push(e),
        }
    }
}

#[test]
fn cursor_turn_streams_init_thinking_tools_text_and_result() {
    let f = bound();
    let session = start_with(
        &f,
        None,
        "hello cursor",
        Some("composer-1"),
        Mode::Default,
        &[],
    )
    .expect("start");
    let mut env = None;
    let events = turn(session.events(), &mut env);
    assert_eq!(
        events,
        vec![
            Event::Mcp {
                servers: vec![],
                launch: true,
                warning: None,
            },
            Event::Init {
                session_id: "cur_fake_sess_1".into(),
                model: "composer-1".into(),
            },
            Event::Delta {
                kind: DeltaKind::Thinking,
                text: "Inspecting ".into(),
            },
            Event::Delta {
                kind: DeltaKind::Thinking,
                text: "the repo.".into(),
            },
            Event::Assistant {
                content: vec![Block::Thinking {
                    thinking: "Inspecting the repo.".into(),
                }],
            },
            Event::Assistant {
                content: vec![Block::ToolUse {
                    id: "call_1".into(),
                    name: "Bash".into(),
                    input: serde_json::json!({ "command": "ls -la" }),
                }],
            },
            Event::User {
                content: vec![Block::ToolResult {
                    tool_use_id: "call_1".into(),
                    content: serde_json::json!({ "stdout": "README.md\n", "exitCode": 0 }),
                    is_error: false,
                }],
            },
            Event::Delta {
                kind: DeltaKind::Text,
                text: "Hello from ".into(),
            },
            Event::Delta {
                kind: DeltaKind::Text,
                text: "fake cursor.".into(),
            },
            Event::Assistant {
                content: vec![Block::Text {
                    text: "Hello from fake cursor.".into(),
                }],
            },
            Event::Result {
                is_error: false,
                duration_ms: 42,
                total_cost_usd: 0.0,
                result: Some("Hello from fake cursor.".into()),
                prompts: 1,
                usage: Some(Usage {
                    input: 120,
                    output: 30,
                }),
                turn_cost_usd: None,
            },
        ]
    );
    let env = env.expect("fake_env");
    assert_eq!(
        env["cursor_config_dir"],
        f.dir.join("cursor-home").to_str().expect("utf-8")
    );
    assert_eq!(env["prompt"], "hello cursor");
    assert_eq!(env["force"], false);
    assert_eq!(env["mode"], "");
    let args = env["args"].as_str().expect("args");
    assert!(args.contains("--output-format stream-json"), "{args}");
    assert!(args.contains("--stream-partial-output"), "{args}");
    assert!(args.contains("--model composer-1"), "{args}");
}

#[test]
fn cursor_maps_flat_tools_modes_resume_and_session_gone() {
    let f = bound();
    let s = start_with(&f, Some("cur_prev_9"), "flat_tool", None, Mode::Plan, &[]).expect("start");
    let mut env = None;
    let events = turn(s.events(), &mut env);
    let env = env.expect("env");
    assert_eq!(env["mode"], "plan");
    assert_eq!(env["force"], false);
    assert!(
        env["args"]
            .as_str()
            .expect("args")
            .contains("--resume cur_prev_9")
    );
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Init { session_id, .. } if session_id == "cur_prev_9"
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Assistant { content }
            if matches!(&content[..], [Block::ToolUse { id, name, .. }] if id == "call_flat_1" && name == "grep_search")
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::User { content }
            if matches!(&content[..], [Block::ToolResult { tool_use_id, is_error: false, .. }] if tool_use_id == "call_flat_1")
    )));

    // AcceptEdits also omits --force (no silent escalation to full-access).
    let s = start_with(&f, None, "hi", None, Mode::AcceptEdits, &[]).expect("accept edits");
    let mut env = None;
    turn(s.events(), &mut env);
    assert_eq!(env.expect("env")["force"], false);

    // Bypass passes --force.
    let s = start_with(&f, None, "hi", None, Mode::Bypass, &[]).expect("bypass");
    let mut env = None;
    turn(s.events(), &mut env);
    assert_eq!(env.expect("env")["force"], true);

    // Flag-like resume id refused.
    assert!(start_with(&f, Some("--force"), "hi", None, Mode::Default, &[]).is_err());

    // Missing resumed session emits Event::SessionGone.
    std::fs::write(f.dir.join(".fake-session-gone"), "").expect("marker");
    let s = start_with(&f, Some("cur_missing"), "hi", None, Mode::Default, &[]).expect("start");
    let mut env = None;
    let events = turn(s.events(), &mut env);
    assert!(events.contains(&Event::SessionGone), "{events:?}");
}

#[test]
fn cursor_interrupt_ends_hanging_turn() {
    let f = bound();
    let mut session = start_with(&f, None, "hang please", None, Mode::Default, &[]).expect("start");
    let rx = session.events().clone();
    // Wait for Init + thinking delta so the child is running and trapping SIGINT.
    loop {
        if matches!(next(&rx), Event::Delta { .. }) {
            break;
        }
    }
    session.interrupt().expect("interrupt");
    let mut env = None;
    let events = turn(&rx, &mut env);
    assert!(
        matches!(
            events.last(),
            Some(Event::Result {
                is_error: true,
                result: Some(r),
                ..
            }) if r == "interrupted"
        ),
        "{events:?}"
    );
}

#[test]
fn cursor_accepts_text_attachments_and_refuses_binary_ones() {
    let f = bound();
    let txt = Attachment::new("notes.txt", b"hello file".to_vec()).expect("txt");
    let s = start_with(&f, None, "check this", None, Mode::Default, &[txt]).expect("text ok");
    let mut env = None;
    turn(s.events(), &mut env);
    let prompt = env.expect("env")["prompt"]
        .as_str()
        .expect("prompt")
        .to_owned();
    assert!(
        prompt.contains("notes.txt") && prompt.contains("hello file"),
        "{prompt}"
    );

    let pdf = Attachment::new("doc.pdf", b"%PDF-1.7".to_vec()).expect("pdf");
    let err = start_with(&f, None, "read pdf", None, Mode::Default, &[pdf])
        .err()
        .expect("pdf refused");
    assert!(err.to_string().contains("Cursor"), "{err:#}");

    let png = Attachment::new("img.png", b"\x89PNG\r\n\x1a\n".to_vec()).expect("png");
    let err = start_with(&f, None, "read png", None, Mode::Default, &[png])
        .err()
        .expect("png refused");
    assert!(err.to_string().contains("Cursor"), "{err:#}");
}

#[test]
fn cursor_bin_resolves_cursor_agent_then_falls_back_to_agent() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
    let agent = dir.join("agent");
    std::fs::write(&agent, "#!/bin/sh\nexit 0\n").expect("write");
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let resolved =
        skyelab_agent::resolve_cursor_bin_in(Path::new("cursor-agent"), std::slice::from_ref(&dir))
            .expect("fallback to agent");
    assert_eq!(resolved, agent);

    let cursor_agent = dir.join("cursor-agent");
    std::fs::write(&cursor_agent, "#!/bin/sh\nexit 0\n").expect("write");
    std::fs::set_permissions(&cursor_agent, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let resolved = skyelab_agent::resolve_cursor_bin_in(Path::new("cursor-agent"), &[dir])
        .expect("prefers cursor-agent");
    assert_eq!(resolved, cursor_agent);
}
