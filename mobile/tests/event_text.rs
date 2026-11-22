//! The thread screen reads events in the shape the desktop stores them: `prompt` rows hold a JSON string, `agent`
//! rows hold `skyelab_agent::Event` serialized as serde's default externally tagged enum.

use skyelab_mobile::app::event_text;
use skyelab_protocol::ThreadEvent;

fn row(kind: &str, payload: &str) -> ThreadEvent {
    ThreadEvent {
        thread_id: 1,
        seq: 1,
        kind: kind.into(),
        payload_json: payload.into(),
        created_at: 0,
    }
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
            "→ Bash: cargo test",
        ),
        (
            "agent",
            r#"{"User":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}]}}"#,
            "(tool output)",
        ),
        (
            "agent",
            r#"{"Result":{"is_error":false,"duration_ms":5,"total_cost_usd":0.0,"result":"All done"}}"#,
            "All done",
        ),
    ] {
        assert_eq!(event_text(&row(kind, payload)), want, "{payload}");
    }
}
