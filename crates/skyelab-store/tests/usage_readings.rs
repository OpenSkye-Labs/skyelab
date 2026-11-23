//! Acceptance: each account's newest usage reading per window is kept as events arrive (a trigger), so the Details
//! tab reads a handful of rows instead of searching every event's text. Migration 10 backfills older databases.

mod common;

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};
use std::path::Path;

fn thread(s: &mut Store, project: i64, account_id: &str) -> i64 {
    s.create_thread(&NewThread {
        project_id: project,
        title: "t",
        cwd: Path::new("/demo"),
        account_id,
        status: "idle",
    })
    .expect("thread")
    .id
}

fn limit(window: &str, used: Option<f32>, limited: bool) -> String {
    let used = used.map_or(String::new(), |u| format!(r#","used":{u}"#));
    let window = if window.is_empty() {
        String::new()
    } else {
        format!(r#","window":"{window}""#)
    };
    format!(r#"{{"RateLimit":{{"limited":{limited},"resets_at":1791804000{window}{used}}}}}"#)
}

/// window -> payload, for one account.
fn readings(s: &Store, account: &str) -> Vec<(String, String)> {
    let mut r: Vec<(String, String)> = s
        .usage_readings(account)
        .expect("readings")
        .into_iter()
        .map(|r| (r.window, r.payload_json))
        .collect();
    r.sort();
    r
}

#[test]
fn newest_reading_per_window_and_account_is_kept() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s
        .create_project("demo", Path::new("/demo"))
        .expect("project")
        .id;
    let (work, home) = (thread(&mut s, p, "work"), thread(&mut s, p, "work"));
    let other = thread(&mut s, p, "home");
    s.append_event(work, "agent", &limit("five_hour", Some(0.25), false))
        .expect("append");
    s.append_event(home, "agent", &limit("seven_day", Some(0.5), false))
        .expect("append");
    s.append_event(work, "agent", &limit("five_hour", Some(0.75), false))
        .expect("append");
    // A status-only line: newest overall, but it doesn't erase the 5h percentage.
    s.append_event(work, "agent", &limit("five_hour", None, true))
        .expect("append");
    // Not a reading: another event kind, and text that only mentions RateLimit.
    s.append_event(work, "prompt", &limit("five_hour", Some(0.9), false))
        .expect("append");
    s.append_event(work, "agent", r#"{"Unknown":"RateLimit used"}"#)
        .expect("append");
    s.append_event(other, "agent", &limit("five_hour", Some(0.1), false))
        .expect("append");

    assert_eq!(
        readings(&s, "work"),
        vec![
            (String::new(), limit("five_hour", None, true)),
            ("five_hour".into(), limit("five_hour", Some(0.75), false)),
            ("seven_day".into(), limit("seven_day", Some(0.5), false)),
        ]
    );
    assert_eq!(readings(&s, "home").len(), 2, "{:?}", readings(&s, "home"));
    assert!(readings(&s, "nobody").is_empty());
}

#[test]
fn migration_backfills_readings_from_stored_events() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    {
        let mut s = Store::open(&path).expect("store");
        let p = s
            .create_project("demo", Path::new("/demo"))
            .expect("project")
            .id;
        let t = thread(&mut s, p, "work");
        s.append_event(t, "agent", &limit("seven_day", Some(0.25), false))
            .expect("append");
        s.append_event(t, "agent", &limit("seven_day", Some(0.5), false))
            .expect("append");
    }
    // Back to version 9: the table and trigger as they were before.
    let conn = Connection::open(&path).expect("raw");
    conn.execute_batch(common::UNDO_V21).expect("drop v21");
    conn.execute_batch(common::UNDO_V19).expect("drop v19");
    conn.execute_batch(common::UNDO_V16).expect("drop v16");
    conn.execute_batch(common::UNDO_V15).expect("drop v15");
    conn.execute_batch(common::UNDO_V14).expect("drop v14");
    conn.execute_batch(common::UNDO_V13).expect("drop v13");
    conn.execute_batch(common::UNDO_V11).expect("drop v11");
    conn.execute_batch("DROP TRIGGER usage_reading_insert; DROP TABLE usage_readings;")
        .expect("drop v10");
    conn.pragma_update(None, "user_version", 9)
        .expect("set version");
    drop(conn);

    let mut s = Store::open(&path).expect("migrates");
    assert_eq!(
        readings(&s, "work"),
        vec![
            (String::new(), limit("seven_day", Some(0.5), false)),
            ("seven_day".into(), limit("seven_day", Some(0.5), false)),
        ]
    );
    // v11 ran after v10's backfill: a steered message is searchable.
    let t = s.list_all_threads().expect("threads")[0].id;
    s.append_event(t, "steer", "\"mind the zebra\"")
        .expect("append");
    let found = s.search_threads("zebra", None, 10).expect("search");
    assert_eq!(found.iter().map(|f| f.id).collect::<Vec<_>>(), [t]);
}
