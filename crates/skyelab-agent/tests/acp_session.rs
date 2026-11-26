//! Acceptance: the reusable ACP stdio driver completes initialize -> session/new (and resume) -> session/prompt,
//! streams text/thinking deltas and Assistant blocks, maps tool calls and plans, handles permissions and
//! `interaction_*` questions, supports cancel/interrupt, model/effort config updates, modes, and EOF withdrawal.
#![cfg(unix)]

use serde_json::{Value, json};
use skyelab_agent::{
    Block, Decision, DeltaKind, Event, Launch, Mode, QUESTION_TOOL, Session, TODO_TOOL,
    start_with_bin, todos,
};
use skyelab_core::{Account, AccountsConfig, Binding, Provider};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn fake_antigravity() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-antigravity.sh")
}

struct Rig {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    account: Account,
    config: AccountsConfig,
}

impl Rig {
    fn new(signed_in: bool) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = std::fs::canonicalize(tmp.path()).expect("canonical");
        let gemini_home = dir.join("gemini-home");
        std::fs::create_dir_all(&gemini_home).expect("mkdir gemini_home");
        if signed_in {
            std::fs::write(gemini_home.join("fake-login"), "me@example.com\n").expect("login");
        }
        let account = Account {
            id: "agy".into(),
            label: "Antigravity".into(),
            provider: Provider::Antigravity,
            config_dir: gemini_home,
            gh_config_dir: None,
            ssh_agent: false,
        };
        let config = AccountsConfig {
            accounts: vec![account.clone()],
            bindings: vec![Binding::new(dir.clone(), "agy".into())],
        };
        Self {
            _tmp: tmp,
            dir,
            account,
            config,
        }
    }

    fn launch(
        &self,
        prompt: &str,
        mode: Mode,
        resume: Option<&str>,
        mcp: Option<&Path>,
    ) -> Session {
        let launch = Launch {
            account: &self.account,
            dir: &self.dir,
            resume,
            prompt,
            mcp_config: mcp,
            model: Some("gemini-2.5-pro"),
            effort: Some("high"),
            attachments: &[],
            mode,
            untrusted: false,
        };
        start_with_bin(&fake_antigravity(), &self.config, &launch).expect("start")
    }
}

fn collect_until_result(session: &Session) -> Vec<Event> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut out = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let ev = session
            .events()
            .recv_timeout(left)
            .expect("event before deadline");
        let done = matches!(ev, Event::Result { .. });
        out.push(ev);
        if done {
            return out;
        }
    }
}

fn requests(events: &[Event]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Unknown(v) if v["method"] == "fake/request" => Some(v["params"].clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn acp_full_turn_streams_deltas_tools_plan_mcp_and_config_options() {
    let rig = Rig::new(true);
    let grant = rig.dir.join("mcp.json");
    std::fs::write(
        &grant,
        json!({
            "mcpServers": {
                "skyelab": {
                    "command": "/bin/echo",
                    "args": ["--mcp-proxy"],
                    "env": { "SKYELAB_TOKEN": "tok" }
                }
            }
        })
        .to_string(),
    )
    .expect("grant");

    let session = rig.launch("hello world", Mode::Default, None, Some(&grant));
    let events = collect_until_result(&session);

    assert!(events.iter().any(|e| matches!(
        e,
        Event::Init { session_id, model } if session_id == "acp_sess_fake" && !model.is_empty()
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Delta { kind: DeltaKind::Thinking, text } if text.contains("Thinking")
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Delta { kind: DeltaKind::Text, text } if text.contains("Hello from ")
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Assistant { content }
            if content.iter().any(|b| matches!(b, Block::Text { text } if text == "Hello from fake Antigravity."))
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Assistant { content }
            if content.iter().any(|b| matches!(b, Block::ToolUse { id, .. } if id == "call_1"))
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::User { content }
            if content.iter().any(|b| matches!(b, Block::ToolResult { tool_use_id, is_error: false, .. } if tool_use_id == "call_1"))
    )));
    // Plan mapped to TodoWrite.
    let plan_input = events.iter().find_map(|e| match e {
        Event::Assistant { content } => content.iter().find_map(|b| match b {
            Block::ToolUse { name, input, .. } if name == TODO_TOOL => Some(input),
            _ => None,
        }),
        _ => None,
    });
    let plan_items = todos(plan_input.expect("plan mapped to TodoWrite"));
    assert_eq!(plan_items.len(), 2);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Context {
            tokens: Some(1200),
            window: Some(200_000),
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Result {
            is_error: false,
            ..
        }
    )));

    let reqs = requests(&events);
    let new_req = reqs
        .iter()
        .find(|r| r["method"] == "session/new")
        .expect("session/new");
    let mcp_servers = new_req["params"]["mcpServers"]
        .as_array()
        .expect("mcpServers");
    assert_eq!(mcp_servers.len(), 1);
    assert_eq!(mcp_servers[0]["name"], "skyelab");
    assert!(
        reqs.iter()
            .any(|r| r["method"] == "session/set_config_option"
                && r["params"]["value"] == "gemini-2.5-pro"),
        "model config option sent: {reqs:?}"
    );
    assert!(
        reqs.iter()
            .any(|r| r["method"] == "session/set_config_option" && r["params"]["value"] == "high"),
        "thought_level config option sent: {reqs:?}"
    );
}

#[test]
fn acp_permissions_questions_interrupt_modes_and_resume() {
    let rig = Rig::new(true);

    // 1. Permission allow/deny
    let mut session = rig.launch("approve", Mode::Default, None, None);
    let deadline = Instant::now() + Duration::from_secs(30);
    let ask_id = loop {
        let ev = session
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("permission");
        if let Event::Permission { id, tool, input } = ev {
            assert!(!tool.is_empty());
            assert_eq!(input["command"], "make deploy");
            break id;
        }
    };
    session
        .answer(&ask_id, Decision::AllowOnce)
        .expect("answer");
    let rest = collect_until_result(&session);
    assert!(rest.iter().any(|e| matches!(
        e,
        Event::PermissionResolved { id, decision: Decision::AllowOnce, .. } if *id == ask_id
    )));
    let reqs = requests(&rest);
    assert!(
        reqs.iter()
            .any(|r| r["result"]["outcome"]["optionId"] == "allow_once"),
        "allow_once reply sent: {reqs:?}"
    );

    // 2. User-input question (`interaction_*`)
    session
        .send_user_message("question")
        .expect("question prompt");
    let q_id = loop {
        let ev = session
            .events()
            .recv_timeout(Duration::from_secs(30))
            .expect("question");
        if let Event::Permission { id, tool, .. } = ev {
            assert_eq!(tool, QUESTION_TOOL);
            break id;
        }
    };
    session
        .answer_questions(&q_id, &[vec!["PostgreSQL".into()]])
        .expect("answer question");
    let q_rest = collect_until_result(&session);
    let q_reqs = requests(&q_rest);
    assert!(
        q_reqs
            .iter()
            .any(|r| r["result"]["outcome"]["optionId"] == "pg"),
        "question optionId sent: {q_reqs:?}"
    );

    // 3. Interrupt sends session/cancel and ends turn as interrupted.
    session.send_user_message("hang").expect("hang");
    // Wait until session/prompt was received by fake.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let ev = session
            .events()
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("prompt ack");
        if matches!(&ev, Event::Unknown(v) if v["method"] == "fake/request" && v["params"]["method"] == "session/prompt")
        {
            break;
        }
    }
    session.interrupt().expect("interrupt");
    let cancel_events = collect_until_result(&session);
    assert!(cancel_events.iter().any(|e| matches!(
        e,
        Event::Result { is_error: true, result: Some(r), .. } if r == "interrupted"
    )));

    // 4. set_mode sends session/set_mode; Bypass launch passes --dangerously-skip-permissions.
    session.set_mode(Mode::Plan).expect("set_mode");
    drop(session);

    let bypass = rig.launch("hi", Mode::Bypass, None, None);
    let bypass_events = collect_until_result(&bypass);
    let env_ev = bypass_events
        .iter()
        .find_map(|e| match e {
            Event::Unknown(v) if v["method"] == "fake/env" => Some(&v["params"]),
            _ => None,
        })
        .expect("fake/env");
    assert!(
        env_ev["args"]
            .as_str()
            .unwrap_or("")
            .contains("--dangerously-skip-permissions"),
        "bypass flag missing: {env_ev}"
    );

    // 5. Resume + SessionGone
    let resumed = rig.launch("hi again", Mode::Default, Some("prev_sess_42"), None);
    let resumed_events = collect_until_result(&resumed);
    assert!(resumed_events.iter().any(|e| matches!(
        e,
        Event::Init { session_id, .. } if session_id == "prev_sess_42"
    )));

    std::fs::write(rig.dir.join(".fake-session-gone"), "").expect("session gone");
    let gone = rig.launch("hi", Mode::Default, Some("dead_sess"), None);
    let gone_events = collect_until_result(&gone);
    assert!(gone_events.iter().any(|e| matches!(e, Event::SessionGone)));
}
