//! A turn stopped before its result (Stop, a crash) has no divider. It must not fold into the next turn's header.

use super::thread_model::{Item, ToolItem};
use super::timeline_fold::{fold_head, folded_by};

fn tool(id: &str) -> Item {
    Item::Tool(ToolItem {
        id: id.into(),
        name: "Bash".into(),
        input: "{}".into(),
        raw: serde_json::json!({}),
        result: Some("ok".into()),
        is_error: false,
    })
}

#[test]
fn a_stopped_turn_does_not_fold_into_the_next_one() {
    let items = vec![
        Item::Prompt("go".into()),
        tool("a"),
        Item::Text("half an answer".into()),
        Item::Prompt("again".into()),
        tool("b"),
        Item::Text("done".into()),
        Item::Done {
            is_error: false,
            duration_ms: 4200,
            cost_usd: 0.0,
            total_usd: 0.0,
            usage: None,
            at_ms: Some(1),
            error: None,
        },
    ];
    assert_eq!(
        folded_by(&items, 1),
        None,
        "stopped turn's tool folded under the next turn's header"
    );
    assert_eq!(
        folded_by(&items, 2),
        None,
        "stopped turn's last text hidden"
    );
    assert_eq!(
        fold_head(&items, 6),
        Some(4),
        "header belongs above the resumed turn's work"
    );
}
