//! A tool row's one-line summary is the first plain value of its input, as T3 shows it.

use super::thread_items::tool_summary;
use super::thread_model::ToolItem;
use serde_json::{Value, json};

fn tool(input: Value) -> ToolItem {
    ToolItem {
        id: "t".into(),
        name: "x".into(),
        input: serde_json::to_string_pretty(&input).expect("json"),
        raw: input,
        result: None,
        is_error: false,
    }
}

#[test]
fn the_summary_skips_nested_lists_for_their_first_plain_value() {
    assert_eq!(tool_summary(&tool(json!({"command": "ls crates"}))), "ls crates");
    // Real Claude's AskUserQuestion input: its row read `[` while the question waited.
    let ask = json!({"questions": [{"question": "Which color?", "header": "color",
        "options": [{"label": "red"}]}]});
    assert_eq!(tool_summary(&tool(ask)), "Which color?");
    assert_eq!(tool_summary(&tool(json!({"todos": []}))), "");
}
