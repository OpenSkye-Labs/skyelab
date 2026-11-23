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
    assert_eq!(v, 12);
    drop(conn);
    let store = Store::open(&path).expect("reopen");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    store
        .set_project_model(p.id, &pick("opus", "max"))
        .expect("set");
}
