//! A screenshot in a tool result, stored the way the desktop stores it (the store drops the bytes), reads as its
//! size and "not kept" on the phone, as it does on the desktop.

use skyelab_agent::{Block, Event};
use skyelab_mobile::app::event_text;
use skyelab_protocol::ThreadEvent;
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
    assert!(!row.payload_json.contains("BwcH"), "the store dropped the bytes");
    let event = ThreadEvent {
        thread_id: row.thread_id,
        seq: row.seq,
        kind: row.kind,
        payload_json: row.payload_json,
        created_at: row.created_at,
    };
    assert_eq!(event_text(&event), "(tool output)\nImage (205 KB) not kept");
}
