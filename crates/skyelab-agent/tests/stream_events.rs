#![cfg(unix)]
//! Claude's richer stream (partial messages, thinking, usage, rate limits, questions, a gone session) through a real
//! session on the fake: what reaches the events, and what the agent gets back. The stream is a recorded
//! `--include-partial-messages` turn (Claude Code 2.1.282, from T3 Code's transcripts).

use serde_json::{Value, json};
use skyelab_agent::{
    Block, DeltaKind, Event, Launch, Mode, Session, TodoStatus, Usage, start_with_bin, todos,
};
use skyelab_core::{Account, AccountsConfig, Binding};
use std::path::Path;
use std::time::Duration;

fn start(prompt: &str) -> (Session, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::fs::canonicalize(tmp.path()).expect("canonical tempdir");
    let account = Account {
        id: "acme".into(),
        label: "Acme".into(),
        config_dir: dir.join("account"),
        provider: Default::default(),
        gh_config_dir: None,
        ssh_agent: false,
    };
    let cfg = AccountsConfig {
        accounts: vec![account.clone()],
        bindings: vec![Binding::new(dir.clone(), "acme".into())],
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
    let bin = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh");
    (
        start_with_bin(&bin, &cfg, &launch).expect("start fake"),
        tmp,
    )
}

/// Every event up to and including the first whose `stop` is true.
fn until(s: &Session, stop: impl Fn(&Event) -> bool) -> Vec<Event> {
    let mut out = Vec::new();
    loop {
        let e = s
            .events()
            .recv_timeout(Duration::from_secs(10))
            .expect("event within 10s");
        let done = stop(&e);
        out.push(e);
        if done {
            return out;
        }
    }
}

fn is_result(e: &Event) -> bool {
    matches!(e, Event::Result { .. })
}

/// The recorded turn, from the prompt's echo on, and the args the fake was started with.
fn streamed() -> (Vec<Event>, String) {
    let (s, _tmp) = start("stream please");
    let before = until(
        &s,
        |e| matches!(e, Event::Unknown(v) if v["type"] == "fake_env"),
    );
    let args = match before.last() {
        Some(Event::Unknown(v)) => v["args"].as_str().unwrap_or_default().to_owned(),
        _ => String::new(),
    };
    // session.jsonl (and its result) came before fake_env.
    (until(&s, is_result), args)
}

#[test]
fn claude_is_asked_for_partial_messages() {
    let (_, args) = streamed();
    assert!(args.contains("--include-partial-messages"), "{args}");
}

#[test]
fn deltas_thinking_and_usage_parse_and_chatter_is_dropped() {
    let (events, _) = streamed();
    let deltas = |kind| -> String {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Delta { kind: k, text } if *k == kind => Some(text.as_str()),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        deltas(DeltaKind::Thinking),
        "The user wants me to run a specific shell command in the foreground and wait for it to finish, then respond with a specific message."
    );
    assert_eq!(
        deltas(DeltaKind::Text),
        "Running the specified command now.command output fixture complete"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Assistant { content }
        if matches!(&content[..], [Block::Thinking { thinking }] if thinking.starts_with("The user wants")))));
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::RateLimit {
                limited: false,
                resets_at: None,
                ..
            }
        )),
        "{events:?}"
    );
    // Block starts and stops, tool input JSON, message deltas, thinking token counts and status pings are dropped.
    let chatter: Vec<&Value> = events
        .iter()
        .filter_map(|e| match e {
            Event::Unknown(v) => Some(v),
            _ => None,
        })
        .filter(|v| {
            v["type"] == "stream_event"
                || v["subtype"] == "thinking_tokens"
                || v["subtype"] == "status"
        })
        .collect();
    assert!(chatter.is_empty(), "{chatter:?}");
    // A background task's lines are task events (a Bash task here: no subagent card takes them).
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Task { status: Some(s), .. } if s == "running"))
    );
    match events.last() {
        Some(Event::Result { usage, .. }) => assert_eq!(
            *usage,
            Some(Usage {
                input: 4 + 13823 + 256120,
                output: 175
            })
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_subagents_deltas_are_dropped() {
    let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}},"parent_tool_use_id":"toolu_1"}"#;
    assert!(matches!(Event::parse(line), Event::Unknown(_)));
    let root = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}},"parent_tool_use_id":null}"#;
    assert_eq!(
        Event::parse(root),
        Event::Delta {
            kind: DeltaKind::Text,
            text: "x".into()
        }
    );
}

#[test]
fn a_rejected_rate_limit_is_limited_unless_overage_allows_it() {
    let rejected = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","rateLimitType":"five_hour","resetsAt":1791804000}}"#;
    assert_eq!(
        Event::parse(rejected),
        Event::RateLimit {
            limited: true,
            resets_at: Some(1791804000),
            window: Some("five_hour".into()),
            used: None,
        }
    );
    let overage = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1,"overageStatus":"allowed"}}"#;
    assert!(matches!(
        Event::parse(overage),
        Event::RateLimit { limited: false, .. }
    ));
}

#[test]
fn todo_write_input_parses() {
    let input = json!({ "todos": [
        { "content": "Read the code", "status": "completed", "activeForm": "Reading" },
        { "content": "Write it", "status": "in_progress" },
        { "content": "Ship", "status": "pending" },
        { "content": "  ", "status": "pending" },
    ]});
    let t = todos(&input);
    assert_eq!(t.len(), 3);
    assert_eq!(t[0].content, "Read the code");
    assert_eq!(
        t.iter().map(|t| t.status).collect::<Vec<_>>(),
        [
            TodoStatus::Completed,
            TodoStatus::InProgress,
            TodoStatus::Pending
        ]
    );
}

#[test]
fn a_missing_resumed_session_says_so_before_its_result() {
    let (s, _tmp) = start("hi");
    until(&s, is_result);
    let mut s = s;
    s.send_user_message("gone now").expect("send");
    let events = until(&s, is_result);
    let n = events.len();
    assert!(n >= 2, "{events:?}");
    assert_eq!(events[n - 2], Event::SessionGone);
    assert!(skyelab_agent::session_gone(&events[n - 2]));
    assert!(skyelab_agent::session_gone(&Event::Stderr {
        text: "Error: No conversation found with session ID: abc".into()
    }));
}

#[test]
fn ask_user_question_answers_reach_the_agent() {
    let (mut s, _tmp) = start("question now");
    let (id, tool) = until(&s, |e| matches!(e, Event::Permission { .. }))
        .into_iter()
        .find_map(|e| match e {
            Event::Permission { id, tool, .. } => Some((id, tool)),
            _ => None,
        })
        .expect("asked");
    assert_eq!(tool, skyelab_agent::QUESTION_TOOL);
    assert!(
        s.answer_questions(&id, &[vec!["Postgres".into()]]).is_err(),
        "a short answer list must be refused"
    );
    s.answer_questions(
        &id,
        &[
            vec!["SQLite".into()],
            vec!["Cache".into(), "Redis please".into()],
        ],
    )
    .expect("answer");
    let reply = until(
        &s,
        |e| matches!(e, Event::Unknown(v) if v["type"] == "fake_answer"),
    );
    let Some(Event::Unknown(v)) = reply.last() else {
        panic!("no reply")
    };
    let r = &v["reply"]["response"];
    assert_eq!(r["request_id"], "q-1");
    assert_eq!(r["response"]["behavior"], "allow");
    assert_eq!(
        r["response"]["updatedInput"]["answers"],
        json!({ "Which database?": "SQLite", "Which extras?": "Cache, Redis please" })
    );
    assert_eq!(
        r["response"]["updatedInput"]["questions"][1]["header"],
        "Extras"
    );
    assert!(
        reply
            .iter()
            .any(|e| matches!(e, Event::PermissionResolved { id: i, .. } if *i == id))
    );
}
