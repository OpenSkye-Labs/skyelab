//! Acceptance: every stored turn result's cost and tokens are kept per account and project as events arrive (a
//! trigger, backfilled by migration 16), and the Usage page's sums come from a bounded, time-indexed query.

use rusqlite::Connection;
use skyelab_store::usage::{self, Range, Spend};
use skyelab_store::{NewThread, Store};
use std::path::Path;

const DAY: i64 = 86_400_000;

fn thread(s: &Store, project: i64, account_id: &str) -> i64 {
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

fn result(cost: f64, input: u64, output: u64) -> String {
    format!(
        r#"{{"Result":{{"is_error":false,"duration_ms":5,"total_cost_usd":{cost},"result":"ok","prompts":1,"usage":{{"input":{input},"output":{output}}}}}}}"#
    )
}

/// Dates every stored turn of `thread` at `at` (the trigger dates them when they arrive).
fn age(path: &Path, thread: i64, at: i64) {
    let conn = Connection::open(path).expect("open");
    conn.execute(
        "UPDATE turn_usage SET created_at = ?2 WHERE thread_id = ?1",
        rusqlite::params![thread, at],
    )
    .expect("age");
}

#[test]
fn results_are_summed_per_account_project_and_range() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("s.db");
    let mut s = Store::open(&path).expect("store");
    let p1 = s.create_project("one", Path::new("/one")).expect("p").id;
    let p2 = s.create_project("two", Path::new("/two")).expect("p").id;
    let (a_today, a_old, b_week) = (
        thread(&s, p1, "a"),
        thread(&s, p2, "a"),
        thread(&s, p1, "b"),
    );
    s.append_event(a_today, "agent", &result(0.5, 100, 10))
        .expect("e");
    s.append_event(a_today, "agent", &result(0.25, 50, 5))
        .expect("e");
    // Not a result: ignored.
    s.append_event(a_today, "prompt", "\"hi\"").expect("e");
    s.append_event(a_old, "agent", &result(2.0, 1000, 100))
        .expect("e");
    s.append_event(b_week, "agent", &result(1.0, 10, 1))
        .expect("e");
    // A Codex result has no cost, and an old row may have no usage.
    s.append_event(
        b_week,
        "agent",
        r#"{"Result":{"is_error":true,"duration_ms":1,"total_cost_usd":0.0,"result":null}}"#,
    )
    .expect("e");

    let now = 100 * DAY + 3_600_000; // 1 h into a day
    let today_start = 100 * DAY;
    age(&path, a_today, now - 60_000);
    age(&path, a_old, today_start - 20 * DAY);
    age(&path, b_week, today_start - 3 * DAY);

    let since = usage::since(today_start);
    let rows = s.usage_days(since).expect("rows");
    assert!(
        rows.len() <= 3,
        "grouped per account, project and day: {rows:?}"
    );

    let spend = |range, account: Option<&str>, project: Option<i64>| {
        usage::spend(&rows, range, |r| {
            account.is_none_or(|a| r.account_id == a) && project.is_none_or(|p| r.project_id == p)
        })
    };
    assert_eq!(
        spend(Range::Today, Some("a"), None),
        Spend {
            cost_usd: 0.75,
            input: 150,
            output: 15,
            turns: 2
        }
    );
    assert_eq!(
        spend(Range::Week, Some("a"), None).turns,
        2,
        "the 20-day-old turn is outside 7 days"
    );
    assert_eq!(spend(Range::Month, Some("a"), None).cost_usd, 2.75);
    assert_eq!(spend(Range::Month, Some("a"), Some(p2)).input, 1000);
    assert_eq!(spend(Range::Today, Some("b"), None), Spend::default());
    assert_eq!(
        spend(Range::Week, Some("b"), Some(p1)),
        Spend {
            cost_usd: 1.0,
            input: 10,
            output: 1,
            turns: 2
        }
    );
    assert_eq!(spend(Range::Month, None, None).turns, 5);

    let daily = usage::daily(&rows, |r| r.account_id == "a");
    assert_eq!(daily.len(), usage::DAYS as usize);
    assert_eq!(
        daily[usage::DAYS as usize - 1].cost_usd,
        0.75,
        "today is the last bar"
    );
    assert_eq!(daily[usage::DAYS as usize - 21].cost_usd, 2.0);

    // Older than 30 days: not read at all.
    age(&path, a_old, today_start - 40 * DAY);
    let rows = s.usage_days(since).expect("rows");
    assert!(rows.iter().all(|r| r.project_id != p2));
    drop(s);
}

#[test]
fn imported_history_is_left_out_but_later_turns_count() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s.create_project("p", Path::new("/p")).expect("p").id;
    let new = NewThread {
        project_id: p,
        title: "imported",
        cwd: Path::new("/p"),
        account_id: "a",
        status: "idle",
    };
    let t = s
        .import_thread(&new, "sess", 1_000, &[("agent", result(9.0, 9, 9), 1_000)])
        .expect("import")
        .expect("new")
        .id;
    assert!(
        s.usage_days(0).expect("rows").is_empty(),
        "history isn't new spend"
    );
    s.append_event(t, "agent", &result(1.0, 1, 1)).expect("e");
    let rows = s.usage_days(0).expect("rows");
    assert_eq!(rows.iter().map(|r| r.turns).sum::<u64>(), 1);
}

#[test]
fn a_fork_does_not_count_its_sources_turns_again() {
    let mut s = Store::open_in_memory().expect("store");
    let p = s.create_project("p", Path::new("/p")).expect("p").id;
    let new = NewThread {
        project_id: p,
        title: "t",
        cwd: Path::new("/p"),
        account_id: "a",
        status: "idle",
    };
    let t = s.create_thread(&new).expect("thread").id;
    s.append_event(t, "agent", &result(1.0, 1, 1)).expect("e");
    let events = s.events_since(t, 0).expect("events");
    std::thread::sleep(std::time::Duration::from_millis(2)); // the fork is made after its source's turns
    let fork = s.create_fork(&new, t, "ctx", &events).expect("fork").id;
    let turns = |s: &Store| {
        s.usage_days(0)
            .expect("rows")
            .iter()
            .map(|r| r.turns)
            .sum::<u64>()
    };
    assert_eq!(turns(&s), 1, "the copied history isn't spend");
    std::thread::sleep(std::time::Duration::from_millis(2));
    s.append_event(fork, "agent", &result(1.0, 1, 1))
        .expect("e");
    assert_eq!(turns(&s), 2, "the fork's own turn is");
}

#[test]
fn spend_outlives_a_purged_thread() {
    let s = Store::open_in_memory().expect("store");
    let p = s.create_project("p", Path::new("/p")).expect("p").id;
    let t = thread(&s, p, "a");
    let mut s = s;
    s.append_event(t, "agent", &result(3.0, 1, 1)).expect("e");
    s.delete_thread(t).expect("delete");
    let rows = s.usage_days(0).expect("rows");
    assert_eq!(rows.iter().map(|r| r.turns).sum::<u64>(), 1);
}

#[test]
fn migration_backfills_older_results() {
    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("old.db");
    {
        let s = Store::open(&path).expect("store");
        let p = s.create_project("p", Path::new("/p")).expect("p").id;
        let t = thread(&s, p, "a");
        let mut s = s;
        s.append_event(t, "agent", &result(1.5, 7, 3)).expect("e");
    }
    // Back to version 15: the table goes, as if this database never had it.
    let conn = Connection::open(&path).expect("open");
    conn.execute_batch(
        "DROP TRIGGER turn_usage_insert; DROP TABLE turn_usage; PRAGMA user_version = 15;",
    )
    .expect("downgrade");
    drop(conn);
    let s = Store::open(&path).expect("reopen");
    let rows = s.usage_days(0).expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].cost_usd, rows[0].input, rows[0].output),
        (1.5, 7, 3)
    );
}
