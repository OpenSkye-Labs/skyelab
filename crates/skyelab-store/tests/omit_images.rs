//! Image bytes never reach the store: `append_event` and the v12 migration keep a placeholder (media type, byte
//! size, short hash) instead of the base64 data.

use base64::Engine as _;
use rusqlite::Connection;
use serde_json::{Value, json};
use skyelab_store::{NewThread, Store};

const BYTES: usize = 300_000;

fn b64(len: usize) -> String {
    let bytes: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn thread(store: &Store) -> i64 {
    let p = store
        .create_project("demo", std::path::Path::new("/work/demo"))
        .expect("project");
    store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: &p.path,
            account_id: "acme",
            status: "exited",
        })
        .expect("thread")
        .id
}

/// A Claude tool result that echoes an MCP screenshot, in each image shape an agent sends.
fn tool_result(image: Value) -> String {
    json!({"User": {"content": [{"type": "tool_result", "tool_use_id": "toolu_1", "is_error": false,
        "content": [{"type": "text", "text": "Screenshot 1280x800, 300000 bytes"}, image]}]}})
    .to_string()
}

fn shapes(data: &str) -> Vec<Value> {
    vec![
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": data}}),
        json!({"type": "image", "data": data, "mimeType": "image/png"}),
        json!({"type": "image", "url": format!("data:image/png;base64,{data}")}),
    ]
}

fn image_of(payload: &str) -> Value {
    let v: Value = serde_json::from_str(payload).expect("stored json");
    v["User"]["content"][0]["content"][1].clone()
}

fn assert_placeholder(image: &Value, len: usize) {
    let s = image.to_string();
    assert!(!s.contains("\"data\""), "data kept: {s}");
    assert!(!s.contains("base64,"), "data url kept: {s}");
    let bytes = image["source"]["omitted_bytes"]
        .as_u64()
        .or(image["omitted_bytes"].as_u64());
    assert_eq!(bytes, Some(len as u64), "{s}");
    let hash = image["source"]["sha256"]
        .as_str()
        .or(image["sha256"].as_str())
        .expect("hash");
    assert_eq!(hash.len(), 16);
    assert!(s.contains("image/png"), "media type lost: {s}");
}

#[test]
fn append_event_stores_a_placeholder_for_every_image_shape() {
    let mut store = Store::open_in_memory().expect("store");
    let t = thread(&store);
    let data = b64(BYTES);
    for image in shapes(&data) {
        let ev = store
            .append_event(t, "agent", &tool_result(image))
            .expect("append");
        assert!(ev.payload_json.len() < 1024, "{}", ev.payload_json.len());
        assert_placeholder(&image_of(&ev.payload_json), BYTES);
    }
    for ev in store.events_since(t, 0).expect("read") {
        assert!(ev.payload_json.len() < 1024);
        assert_placeholder(&image_of(&ev.payload_json), BYTES);
        // The text block next to the image is untouched.
        let v: Value = serde_json::from_str(&ev.payload_json).expect("json");
        assert_eq!(
            v["User"]["content"][0]["content"][0]["text"],
            "Screenshot 1280x800, 300000 bytes"
        );
    }
}

#[test]
fn small_and_imageless_payloads_are_stored_as_given() {
    let mut store = Store::open_in_memory().expect("store");
    let t = thread(&store);
    let text = json!({"Assistant": {"content": [{"type": "text", "text": "an image of a cat"}]}})
        .to_string();
    assert_eq!(
        store
            .append_event(t, "agent", &text)
            .expect("a")
            .payload_json,
        text
    );
    assert_eq!(
        store
            .append_event(t, "prompt", "\"hi\"")
            .expect("p")
            .payload_json,
        "\"hi\""
    );
}

#[test]
fn search_never_sees_base64() {
    let mut store = Store::open_in_memory().expect("store");
    let t = thread(&store);
    let data = b64(BYTES);
    store
        .append_event(t, "agent", &tool_result(shapes(&data)[0].clone()))
        .expect("append");
    let needle = &data[1000..1040];
    assert!(
        store
            .search_threads(needle, None, 10)
            .expect("search")
            .is_empty()
    );
}

#[test]
fn migration_v12_rewrites_stored_images_in_old_databases() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    let mut store = Store::open(&path).expect("open");
    let t = thread(&store);
    let kept = store
        .append_event(t, "prompt", "\"look at this\"")
        .expect("prompt");
    drop(store);
    // An older build stored the image raw, at schema v11. Enough rows to cross a batch.
    let conn = Connection::open(&path).expect("raw");
    const SMALL: usize = 20_000;
    let data = b64(SMALL);
    for (i, image) in shapes(&data).into_iter().cycle().take(250).enumerate() {
        conn.execute(
            "INSERT INTO events (thread_id, seq, kind, payload_json, created_at) VALUES (?1, ?2, 'agent', ?3, 0)",
            rusqlite::params![t, i as i64 + 10, tool_result(image)],
        )
        .expect("seed");
    }
    // Undo v19, v16, v15, v14 and v13 too (side chats; turn usage; PR state; imported session; titles, forks, drafts), so the upgrade runs them again.
    conn.execute_batch(
        "ALTER TABLE threads DROP COLUMN limit_resume_at; ALTER TABLE threads DROP COLUMN scroll_mark; DROP INDEX threads_by_compare_group; ALTER TABLE threads DROP COLUMN compare_group; ALTER TABLE threads DROP COLUMN settled_at; ALTER TABLE threads DROP COLUMN snoozed_until; DROP TABLE scheduled_tasks; DROP INDEX threads_scheduled_task; ALTER TABLE threads DROP COLUMN scheduled_task_id; DROP TRIGGER side_chats_delete; ALTER TABLE threads DROP COLUMN side_of; DROP TRIGGER turn_usage_insert; DROP TABLE turn_usage;
         ALTER TABLE threads DROP COLUMN pr_state; ALTER TABLE threads DROP COLUMN imported_session;
         DROP TRIGGER drafts_thread_delete; DROP TABLE drafts; DROP TABLE draft_files;
         ALTER TABLE threads DROP COLUMN title_source; ALTER TABLE threads DROP COLUMN forked_from;
         ALTER TABLE threads DROP COLUMN fork_context; PRAGMA user_version = 11;",
    )
    .expect("downgrade");
    drop(conn);
    let before = std::fs::metadata(&path).expect("size").len();
    assert!(before > 5_000_000, "seeded {before}");

    let store = Store::open(&path).expect("migrates");
    let events = store.events_since(t, 0).expect("read");
    assert_eq!(events.len(), 251);
    assert_eq!(events[0].payload_json, kept.payload_json);
    for ev in &events[1..] {
        assert!(ev.payload_json.len() < 1024, "{}", ev.seq);
        assert_placeholder(&image_of(&ev.payload_json), SMALL);
    }
    drop(store);
    let conn = Connection::open(&path).expect("raw");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(v, 27);
    drop(conn);
    let after = std::fs::metadata(&path).expect("size").len();
    assert!(after < 1_000_000, "file still {after} bytes");
}

#[test]
fn codex_image_generation_and_mcp_resource_blobs_are_dropped() {
    let mut store = Store::open_in_memory().expect("store");
    let t = thread(&store);
    let data = b64(BYTES);
    let imagegen = json!({"Unknown": {"method": "item/completed", "params": {"item": {
        "type": "imageGeneration", "id": "ig_1", "status": "completed", "result": data}}}});
    let blob = json!({"User": {"content": [{"type": "tool_result", "tool_use_id": "x", "is_error": false,
        "content": [{"type": "resource", "resource": {"uri": "shot://1", "mimeType": "image/png", "blob": data}}]}]}});
    for payload in [imagegen, blob] {
        let ev = store
            .append_event(t, "agent", &payload.to_string())
            .expect("append");
        assert!(ev.payload_json.len() < 1024, "{}", ev.payload_json.len());
        assert!(
            ev.payload_json
                .contains(&format!("\"omitted_bytes\":{BYTES}"))
        );
    }
}

#[test]
fn data_that_is_not_base64_is_kept() {
    let mut store = Store::open_in_memory().expect("store");
    let t = thread(&store);
    let id = "node-42:7 is a figma node id, not base64 at all, long enough to pass the floor";
    let payload = json!({"Assistant": {"content": [{"type": "tool_use", "id": "u", "name": "mcp__figma__place",
        "input": {"node": {"type": "image", "data": id}, "note": "x".repeat(2000)}}]}})
    .to_string();
    let ev = store.append_event(t, "agent", &payload).expect("append");
    assert_eq!(ev.payload_json, payload);
}
