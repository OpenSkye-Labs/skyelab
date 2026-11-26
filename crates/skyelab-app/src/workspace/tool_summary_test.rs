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

/// A call waiting on its approval is drawn as waiting, not running; answered, or with another input, it isn't.
#[test]
fn a_call_waits_while_its_approval_is_open() {
    use super::thread_model::{ApprovalItem, Item, Items};
    use skyelab_agent::Decision;
    let call = tool(json!({"command": "make deploy"}));
    let ask = |decision| {
        Item::Approval(ApprovalItem {
            id: "a".into(),
            tool: "Bash".into(),
            summary: String::new(),
            input: String::new(),
            raw: json!({"command": "make deploy", "cwd": "/x"}),
            decision,
            rule: None,
        })
    };
    let items = |a| Items::from(vec![Item::Prompt("go".into()), Item::Tool(call.clone()), a]);
    assert!(super::shell_card::waits(&items(ask(None)), &call));
    assert!(!super::shell_card::waits(&items(ask(Some(Decision::AllowOnce))), &call));
    let other = tool(json!({"command": "ls"}));
    assert!(!super::shell_card::waits(&items(ask(None)), &other));
}
