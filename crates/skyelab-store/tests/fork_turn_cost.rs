//! Acceptance: a fork's turns never take its copied history as their baseline. The fork's first session is fresh, so
//! Claude's running total in its first result is all that turn's own; later turns count from the fork's own totals.
//! Migration 20 fixes fork turns stored before it.

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};
use std::path::Path;
use std::time::Duration;

fn result(total: f64) -> String {
    format!(
        r#"{{"Result":{{"is_error":false,"duration_ms":5,"total_cost_usd":{total},"result":"ok","prompts":1,"usage":{{"input":1,"output":1}}}}}}"#
    )
}

fn cost(s: &Store) -> f64 {
    s.usage_days(0)
        .expect("rows")
        .iter()
        .map(|r| r.cost_usd)
        .sum()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// A thread whose one turn reported 0.5, and its fork, whose turns report `fork_totals`.
fn source_and_fork(s: &mut Store, fork_totals: &[f64]) -> i64 {
    let p = s.create_project("p", Path::new("/p")).expect("p").id;
    let new = NewThread {
        project_id: p,
        title: "t",
        cwd: Path::new("/p"),
        account_id: "a",
        status: "idle",
    };
    let t = s.create_thread(&new).expect("thread").id;
    s.append_event(t, "agent", &result(0.5)).expect("e");
    let events = s.events_since(t, 0).expect("events");
    std::thread::sleep(Duration::from_millis(2)); // the fork is made after its source's turns
    let fork = s.create_fork(&new, t, "ctx", &events).expect("fork").id;
    std::thread::sleep(Duration::from_millis(2));
    for total in fork_totals {
        s.append_event(fork, "agent", &result(*total)).expect("e");
    }
    fork
}

#[test]
fn a_forks_first_turn_counts_its_whole_total() {
    let mut s = Store::open_in_memory().expect("store");
    // 0.75 is over the source's 0.5: with the copied 0.5 as baseline it would count only 0.25.
    source_and_fork(&mut s, &[0.75, 1.0]);
    // The source's 0.5, the fork's first turn 0.75, then its second turn 0.25 over the fork's own 0.75.
    assert!(close(cost(&s), 1.5), "{}", cost(&s));
}

#[test]
fn migration_20_fixes_stored_fork_turns() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("v19.db");
    {
        let mut s = Store::open(&path).expect("store");
        source_and_fork(&mut s, &[0.75]);
    }
    // As version 19 stored it: the fork's first turn counted over the copied 0.5.
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch(
        "UPDATE turn_usage SET cost_usd = 0.25 WHERE thread_id IN (SELECT id FROM threads WHERE forked_from IS NOT NULL);
         DROP TABLE scheduled_tasks; DROP INDEX threads_scheduled_task; ALTER TABLE threads DROP COLUMN scheduled_task_id;
         PRAGMA user_version = 19;",
    )
    .expect("downgrade");
    drop(conn);
    let s = Store::open(&path).expect("reopen");
    assert!(close(cost(&s), 1.25), "{}", cost(&s));
}
