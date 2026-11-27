//! Schema 9: a thread's model and effort, and a project's default for new threads.

use skyelab_store::{ModelChoice, NewThread, Store};
use std::path::Path;

fn pick(model: &str, effort: &str) -> ModelChoice {
    ModelChoice {
        model: Some(model.into()),
        effort: Some(effort.into()),
    }
}

#[test]
fn thread_and_project_models_round_trip() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: Path::new("/work/demo"),
            account_id: "acme",
            status: "idle",
        })
        .expect("thread");
    assert_eq!(
        store.thread_model(t.id).expect("read"),
        ModelChoice::default()
    );
    assert_eq!(
        store.project_model(p.id).expect("read"),
        ModelChoice::default()
    );

    store
        .set_thread_model(t.id, &pick("opus", "high"))
        .expect("set");
    store
        .set_project_model(p.id, &pick("sonnet", "low"))
        .expect("set");
    assert_eq!(
        store.thread_model(t.id).expect("read"),
        pick("opus", "high")
    );
    assert_eq!(
        store.project_model(p.id).expect("read"),
        pick("sonnet", "low")
    );
    // A pick doesn't reorder the sidebar.
    let row = store.thread(t.id).expect("read").expect("row");
    assert_eq!(row.updated_at, t.updated_at);

    store
        .set_thread_model(t.id, &ModelChoice::default())
        .expect("clear");
    assert_eq!(
        store.thread_model(t.id).expect("read"),
        ModelChoice::default()
    );
    assert!(
        store
            .set_thread_model(t.id + 99, &pick("opus", "low"))
            .is_err()
    );
}

#[test]
fn a_new_database_is_at_the_latest_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("s.db");
    drop(Store::open(&path).expect("create"));
    let conn = rusqlite::Connection::open(&path).expect("raw");
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(v, 29);
    drop(conn);
    let store = Store::open(&path).expect("reopen");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    store
        .set_project_model(p.id, &pick("opus", "max"))
        .expect("set");
}

#[test]
fn the_default_model_is_the_newest_unpicked_runs() {
    let mut store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let thread = |store: &Store, account| {
        store
            .create_thread(&NewThread {
                project_id: p.id,
                title: "t",
                cwd: Path::new("/work/demo"),
                account_id: account,
                status: "idle",
            })
            .expect("thread")
            .id
    };
    let init = |m: &str| format!(r#"{{"Init":{{"session_id":"s","model":"{m}"}}}}"#);
    assert_eq!(store.last_default_model("acme").expect("read"), None);
    let plain = thread(&store, "acme");
    store
        .append_event(plain, "agent", &init("claude-opus-5-5"))
        .expect("append");
    store
        .append_event(plain, "prompt", r#"{"text":"\"Init\""}"#)
        .expect("append");
    // A thread with a pick names that pick, not the default; another account's runs don't count.
    let picked = thread(&store, "acme");
    store
        .set_thread_model(picked, &pick("sonnet", "high"))
        .expect("set");
    store
        .append_event(picked, "agent", &init("claude-sonnet-5-5"))
        .expect("append");
    let other = thread(&store, "other");
    store
        .append_event(other, "agent", &init("claude-haiku-5-5"))
        .expect("append");
    assert_eq!(
        store.last_default_model("acme").expect("read").as_deref(),
        Some("claude-opus-5-5")
    );
}
