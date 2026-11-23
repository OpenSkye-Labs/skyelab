//! The thread screen reads events in the shape the desktop stores them: `prompt` rows hold a JSON string, `agent`
//! rows hold `skyelab_agent::Event` serialized as serde's default externally tagged enum.

use skyelab_agent::timeline::{Items, Record, apply};
use skyelab_mobile::timeline_rows::row_lines;

/// What the stored row draws as, open, on top of a Bash call `t1` (so a result has its call).
fn event_text(kind: &str, payload: &str) -> String {
    let mut items = Items::default();
    let call = r#"{"Assistant":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#;
    if payload.contains("tool_result") {
        apply(
            &mut items,
            &Record::from_row("agent", call).expect("call row"),
        );
    }
    apply(
        &mut items,
        &Record::from_row(kind, payload).expect("stored row"),
    );
    row_lines(items.last().expect("a row"), true).join("\n")
}

#[test]
fn stored_agent_rows_render_as_text() {
    for (kind, payload, want) in [
        ("prompt", r#""fix the login""#, "fix the login"),
        (
            "agent",
            r#"{"Assistant":{"content":[{"type":"text","text":"on it"}]}}"#,
            "on it",
        ),
        (
            "agent",
            r#"{"Assistant":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            "$ cargo test",
        ),
        (
            "agent",
            r#"{"User":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}]}}"#,
            "$ cargo test\nok",
        ),
        (
            "agent",
            r#"{"Result":{"is_error":false,"duration_ms":5,"total_cost_usd":0.0,"result":"All done"}}"#,
            "Worked for 0.0s",
        ),
    ] {
        assert_eq!(event_text(kind, payload), want, "{payload}");
    }
}
