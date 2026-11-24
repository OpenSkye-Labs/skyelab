use serde_json::json;
use skyelab_agent::timeline::Record;
use skyelab_agent::turn_result::last_turn;
use skyelab_agent::{Block, Event};

#[test]
fn a_crashed_follow_up_is_not_reported_as_the_previous_success() {
    let records = [
        Record::Prompt("first".into()),
        Record::Agent(Event::Assistant {
            content: vec![Block::ToolUse {
                id: "t".into(),
                name: "Edit".into(),
                input: json!({"file_path": "/r/old.rs"}),
            }],
        }),
        Record::Agent(Event::Result {
            is_error: false,
            duration_ms: 5,
            total_cost_usd: 0.1,
            result: Some("first done".into()),
            prompts: 1,
            usage: None,
            turn_cost_usd: None,
        }),
        Record::Prompt("crash now".into()),
        Record::Agent(Event::Stderr {
            text: "fake-claude: lost the session".into(),
        }),
    ];
    if let Some(t) = last_turn(&records) {
        assert!(
            t.is_error && t.summary != "first done",
            "stale turn reported for a crashed follow-up: {t:?}"
        );
    }
}
