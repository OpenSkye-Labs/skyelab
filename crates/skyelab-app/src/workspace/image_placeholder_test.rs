//! A screenshot in a tool result: the live row names the image and its size; once the store dropped the bytes, the
//! reloaded row says it wasn't kept.

use super::thread_model::{Item, Record, apply};
use base64::Engine as _;
use serde_json::json;
use skyelab_agent::{Block, Event};
use skyelab_store::{NewThread, Store};

fn screenshot() -> Event {
    let png = base64::engine::general_purpose::STANDARD.encode(vec![7u8; 210_039]);
    Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: json!([
                {"type": "text", "text": "Screenshot of the tab"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}}
            ]),
            is_error: false,
        }],
    }
}

fn result_of(record: &Record) -> String {
    let call = Record::Agent(Event::Assistant {
        content: vec![Block::ToolUse {
            id: "toolu_1".into(),
            name: "mcp__skyelab__browser_screenshot".into(),
            input: json!({}),
        }],
    });
    let mut items = Vec::new();
    apply(&mut items, &call);
    apply(&mut items, record);
    match &items[0] {
        Item::Tool(t) => t.result.clone().expect("result"),
        other => panic!("not a tool row: {other:?}"),
    }
}

#[test]
fn live_row_names_the_image_and_a_reloaded_row_says_it_was_not_kept() {
    let live = Record::Agent(screenshot());
    assert_eq!(result_of(&live), "Screenshot of the tab\nImage (205 KB)");

    let mut store = Store::open_in_memory().expect("store");
    let p = store
        .create_project("demo", std::path::Path::new("/work/demo"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: &p.path,
            account_id: "acme",
            status: "exited",
        })
        .expect("thread");
    let (kind, payload) = live.to_row();
    let row = store.append_event(t.id, kind, &payload).expect("append");
    let reloaded = Record::from_row(&row.kind, &row.payload_json).expect("parse");
    assert_eq!(
        result_of(&reloaded),
        "Screenshot of the tab\nImage (205 KB) not kept"
    );
}
