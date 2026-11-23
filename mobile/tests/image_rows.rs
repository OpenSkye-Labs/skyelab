//! A screenshot in a tool result, stored the way the desktop stores it (the store drops the bytes), reads as its
//! size and "not kept" on the phone, as it does on the desktop.

use skyelab_agent::timeline::{Record, apply};
use skyelab_agent::{Block, Event};
use skyelab_mobile::timeline_rows::row_lines;
use skyelab_store::{NewThread, Store};

#[test]
fn a_dropped_image_reads_not_kept() {
    // 210 039 bytes of 7s: the desktop's own screenshot fixture, base64 by hand (each 3 bytes of 7 encode as "BwcH").
    let png = "BwcH".repeat(210_039 / 3);
    let result = Event::User {
        content: vec![Block::ToolResult {
            tool_use_id: "toolu_1".into(),
            content: serde_json::json!([
                {"type": "text", "text": "Screenshot of the tab"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png}}
            ]),
            is_error: false,
        }],
    };
    let mut store = Store::open_in_memory().expect("store");
    let project = store
        .create_project("demo", std::path::Path::new("/work/demo"))
        .expect("project");
    let thread = store
        .create_thread(&NewThread {
            project_id: project.id,
            title: "t",
            cwd: &project.path,
            account_id: "acme",
            status: "exited",
        })
        .expect("thread");
    let payload = serde_json::to_string(&result).expect("serializes");
    let row = store
        .append_event(thread.id, "agent", &payload)
        .expect("append");
    assert!(
        !row.payload_json.contains("BwcH"),
        "the store dropped the bytes"
    );
    let mut items = Vec::new();
    let call = Event::Assistant {
        content: vec![Block::ToolUse {
            id: "toolu_1".into(),
            name: "browser_screenshot".into(),
            input: serde_json::json!({}),
        }],
    };
    apply(&mut items, &Record::Agent(call));
    apply(
        &mut items,
        &Record::from_row(&row.kind, &row.payload_json).expect("stored row"),
    );
    assert_eq!(
        row_lines(&items[0], true).join("\n"),
        "browser_screenshot\nScreenshot of the tab\nImage (205 KB) not kept"
    );
}
