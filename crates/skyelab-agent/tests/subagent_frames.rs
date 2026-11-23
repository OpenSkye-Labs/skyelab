//! Claude's subagent frames (shapes from T3's recorded `subagent` and `claude_nested_subagent_model` transcripts):
//! `task_*` system lines become `Event::Task`, a subagent's own messages `Event::Nested`, and the rest is dropped.

use serde_json::json;
use skyelab_agent::{Block, Event};

fn parse(v: serde_json::Value) -> Event {
    Event::parse(&v.to_string())
}

#[test]
fn task_frames_parse() {
    let started = parse(
        json!({"type":"system","subtype":"task_started","task_id":"a35c","tool_use_id":"toolu_A",
        "description":"Read package.json","subagent_type":"general-purpose","task_type":"local_agent"}),
    );
    assert_eq!(
        started,
        Event::Task {
            tool_use_id: "toolu_A".into(),
            status: Some("running".into()),
            activity: None,
            summary: None,
            duration_ms: None,
        }
    );
    let progress = parse(
        json!({"type":"system","subtype":"task_progress","task_id":"a35c","tool_use_id":"toolu_A",
        "description":"Reading package.json","usage":{"total_tokens":8707,"tool_uses":1,"duration_ms":2053},
        "last_tool_name":"Read"}),
    );
    assert_eq!(
        progress,
        Event::Task {
            tool_use_id: "toolu_A".into(),
            status: None,
            activity: Some("Reading package.json".into()),
            summary: None,
            duration_ms: Some(2053),
        }
    );
    let done = parse(
        json!({"type":"system","subtype":"task_notification","task_id":"a35c","tool_use_id":"toolu_A",
        "status":"completed","summary":"Read package.json","usage":{"tool_uses":1,"duration_ms":6211}}),
    );
    assert_eq!(
        done,
        Event::Task {
            tool_use_id: "toolu_A".into(),
            status: Some("completed".into()),
            activity: None,
            summary: Some("Read package.json".into()),
            duration_ms: Some(6211),
        }
    );
    // A stored Task round-trips.
    let row = serde_json::to_string(&done).expect("serializes");
    assert_eq!(serde_json::from_str::<Event>(&row).expect("parses"), done);
    // Lines nobody shows are dropped by the session's parser (`Event::parse` keeps a dropped line as Unknown).
    for subtype in ["task_updated", "background_tasks_changed"] {
        let line = json!({"type":"system","subtype":subtype,"task_id":"a35c","patch":{"status":"completed"}});
        assert!(
            matches!(parse(line), Event::Unknown(_)),
            "{subtype} is not a task"
        );
    }
}

#[test]
fn nested_frames_carry_their_parent() {
    let call = parse(
        json!({"type":"assistant","parent_tool_use_id":"toolu_A","message":{"role":"assistant",
        "content":[{"type":"tool_use","id":"toolu_R","name":"Read","input":{"file_path":"package.json"}}]}}),
    );
    let Event::Nested { parent, content } = call else {
        panic!("a subagent's call is nested: {call:?}")
    };
    assert_eq!(parent, "toolu_A");
    assert!(matches!(&content[..], [Block::ToolUse { name, .. }] if name == "Read"));
    let result = parse(
        json!({"type":"user","parent_tool_use_id":"toolu_A","message":{"role":"user",
        "content":[{"type":"tool_result","tool_use_id":"toolu_R","content":"1\t{}"}]}}),
    );
    assert!(matches!(result, Event::Nested { ref parent, .. } if parent == "toolu_A"));
    // The subagent's prompt is a plain string: nested, with nothing to show.
    let prompt = parse(json!({"type":"user","parent_tool_use_id":"toolu_A",
        "message":{"role":"user","content":"Read the file"}}));
    assert_eq!(
        prompt,
        Event::Nested {
            parent: "toolu_A".into(),
            content: vec![]
        }
    );
    // The root agent's messages are unchanged.
    let root = parse(
        json!({"type":"assistant","parent_tool_use_id":null,"message":{"role":"assistant",
        "content":[{"type":"text","text":"hi"}]}}),
    );
    assert!(matches!(root, Event::Assistant { .. }));
}
