use serde_json::json;
use skyelab_agent::{Block, Event};

#[test]
fn parses_fixture() {
    let text = include_str!("fixtures/session.jsonl");
    let events: Vec<Event> = text.lines().map(Event::parse).collect();
    assert_eq!(
        events[0],
        Event::Init {
            session_id: "sess-1".into(),
            model: "claude-opus-5-5".into()
        }
    );
    assert_eq!(
        events[1],
        Event::Assistant {
            content: vec![
                Block::Other,
                Block::Text {
                    text: "Listing files.".into()
                },
                Block::ToolUse {
                    id: "toolu_1".into(),
                    name: "Bash".into(),
                    input: json!({"command": "ls"})
                },
            ]
        }
    );
    assert_eq!(
        events[2],
        Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "toolu_1".into(),
                content: json!("a.txt\nb.txt"),
                is_error: false
            }]
        }
    );
    assert!(matches!(&events[3], Event::Unknown(v) if v["type"] == "rate_limit_event"));
    assert_eq!(events[4], Event::Unknown(json!("not json at all")));
    assert_eq!(
        events[5],
        Event::Result {
            is_error: false,
            duration_ms: 1234,
            total_cost_usd: 0.0123,
            result: Some("Done.".into()),
            prompts: 1,
        }
    );
}

#[test]
fn events_round_trip_through_json() {
    let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"hi"},{"type":"tool_use","id":"t","name":"Bash","input":{"command":"ls"}}]}}"#;
    for event in [
        skyelab_agent::Event::parse(line),
        skyelab_agent::Event::parse("not json"),
        skyelab_agent::Event::parse(
            r#"{"type":"result","is_error":true,"duration_ms":1,"result":null}"#,
        ),
    ] {
        let json = serde_json::to_string(&event).expect("serialize");
        assert_eq!(
            serde_json::from_str::<skyelab_agent::Event>(&json).expect("parse"),
            event
        );
    }
}
