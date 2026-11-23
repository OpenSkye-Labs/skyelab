//! Acceptance: a dropped screenshot reads as its size and "not kept"; a usage limit shows as a row and on the thread
//! header until a turn succeeds; each window's latest usage reading shows on the header.

use serde_json::json;
use skyelab_agent::timeline::{Record, apply};
use skyelab_agent::{Block, Event};
use skyelab_mobile::thread_rows::thread_status;
use skyelab_mobile::timeline_rows::row_lines;

fn limit(limited: bool, window: Option<&str>, used: Option<f32>) -> Record {
    Record::Agent(Event::RateLimit {
        limited,
        resets_at: None,
        window: window.map(str::to_owned),
        used,
    })
}

#[test]
fn a_dropped_image_reads_not_kept() {
    let mut items = Vec::new();
    for r in [
        Record::Agent(Event::Assistant {
            content: vec![Block::ToolUse {
                id: "s1".into(),
                name: "browser_screenshot".into(),
                input: json!({}),
            }],
        }),
        // What the desktop store leaves of a screenshot once it drops the bytes.
        Record::Agent(Event::User {
            content: vec![Block::ToolResult {
                tool_use_id: "s1".into(),
                content: json!([{"type": "image", "source": {"type": "base64", "omitted_bytes": 210_039}}]),
                is_error: false,
            }],
        }),
    ] {
        apply(&mut items, &r);
    }
    let open = row_lines(&items[0], true).join("\n");
    assert!(open.contains("Image (205 KB) not kept"), "{open}");
    let closed = row_lines(&items[0], false).join("\n");
    assert!(
        closed.contains("Image (205 KB) not kept"),
        "images show without opening: {closed}"
    );
}

#[test]
fn a_limit_shows_until_a_turn_succeeds() {
    let records = vec![
        limit(false, Some("five_hour"), Some(0.61)),
        limit(false, Some("seven_day"), Some(0.2)),
        limit(true, Some("five_hour"), Some(1.0)),
    ];
    let mut items = Vec::new();
    for r in &records {
        apply(&mut items, r);
    }
    assert!(
        row_lines(items.last().expect("a row"), false)[0].starts_with("Usage limit reached"),
        "the limit row"
    );
    assert_eq!(
        thread_status("running", &records),
        "running · Limited · 5h 100% · 7d 20%"
    );
    assert_eq!(
        thread_status("idle", &records[..2]),
        "idle · 5h 61% · 7d 20%"
    );
    let mut cleared = records.clone();
    cleared.push(Record::Agent(Event::Result {
        is_error: false,
        duration_ms: 1,
        total_cost_usd: 0.0,
        result: None,
        prompts: 1,
        usage: None,
    }));
    assert!(!thread_status("idle", &cleared).contains("Limited"));
}
