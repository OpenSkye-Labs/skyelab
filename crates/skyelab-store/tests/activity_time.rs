//! A thread's `updated_at` is its last activity, as T3's sidebar time is: the user's last message or the agent's last
//! turn end. Bookkeeping (status, session id, title, worktree, PR link) never moves it, so a relaunch that writes
//! statuses and an agent's start-up events can't make every row read "now".

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

const RESULT: &str = r#"{"Result":{"is_error":false,"duration_ms":5,"total_cost_usd":0.0,"result":"done","prompts":1,"usage":null}}"#;

fn new_thread(store: &Store, project_id: i64) -> i64 {
    store
        .create_thread(&NewThread {
            project_id,
            title: "t",
            cwd: Path::new("/work/demo"),
            account_id: "work",
            status: "idle",
        })
        .expect("thread")
        .id
}

fn tick() {
    sleep(Duration::from_millis(5));
}

#[test]
fn only_messages_and_turn_ends_move_a_threads_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("skyelab.db");
    let mut store = Store::open(&path).expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let id = new_thread(&store, p.id);
    let at = |s: &Store| s.thread(id).expect("read").expect("row").updated_at;

    tick();
    let prompt = store.append_event(id, "prompt", "\"hi\"").expect("prompt");
    assert_eq!(at(&store), prompt.created_at, "a message is activity");
    tick();
    let end = store.append_event(id, "agent", RESULT).expect("result");
    assert_eq!(at(&store), end.created_at, "a turn end is activity");

    tick();
    store
        .append_event(id, "agent", r#"{"Init":{"session_id":"s","model":"m"}}"#)
        .expect("init");
    store.set_thread_status(id, "stopped").expect("status");
    store.set_agent_session_id(id, Some("s")).expect("session");
    store.set_thread_title(id, "renamed").expect("title");
    store.set_thread_permission_mode(id, "plan").expect("mode");
    store
        .set_thread_worktree(id, Some(Path::new("/w/t")), Some("b"))
        .expect("worktree");
    store
        .set_thread_pr_url(id, Some("https://example.com/pr/1"))
        .expect("pr");
    assert_eq!(at(&store), end.created_at, "bookkeeping is not activity");
    store.set_thread_status(id, "running").expect("status");
    drop(store);

    // A relaunch marks the running thread interrupted; its time stays.
    tick();
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.thread(id).expect("read").expect("row").status,
        "interrupted"
    );
    assert_eq!(at(&store), end.created_at);
}

#[test]
fn upgrading_dates_each_thread_by_its_last_activity() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("skyelab.db");
    let (busy, quiet, fork, last_msg) = {
        let mut store = Store::open(&path).expect("open");
        let p = store
            .create_project("demo", Path::new("/work/demo"))
            .expect("project");
        let busy = new_thread(&store, p.id);
        store.append_event(busy, "prompt", "\"a\"").expect("prompt");
        tick();
        let last = store.append_event(busy, "steer", "\"b\"").expect("steer");
        tick();
        store
            .append_event(busy, "agent", r#"{"Init":{"session_id":"s","model":"m"}}"#)
            .expect("init");
        let quiet = new_thread(&store, p.id);
        let src = store.thread(busy).expect("read").expect("row");
        let events = store.events_since(busy, 0).expect("events");
        let fork = store
            .create_fork(
                &NewThread {
                    project_id: p.id,
                    title: "fork",
                    cwd: &src.cwd,
                    account_id: "work",
                    status: "stopped",
                },
                busy,
                "ctx",
                &events,
            )
            .expect("fork")
            .id;
        (busy, quiet, fork, last.created_at)
    };
    // The schema before activity dates: every row was last written "now" by bookkeeping.
    let raw = Connection::open(&path).expect("raw");
    raw.execute_batch("UPDATE threads SET updated_at = 9999999999999; PRAGMA user_version = 27;")
        .expect("downgrade");
    drop(raw);

    let store = Store::open(&path).expect("upgrade");
    let row = |id| store.thread(id).expect("read").expect("row");
    assert_eq!(
        row(busy).updated_at,
        last_msg,
        "its last message, not its init"
    );
    assert_eq!(
        row(quiet).updated_at,
        row(quiet).created_at,
        "no activity: its start"
    );
    assert_eq!(
        row(fork).updated_at,
        row(fork).created_at,
        "a fork's copied history is older than the fork"
    );
}
