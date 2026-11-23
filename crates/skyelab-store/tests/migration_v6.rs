//! Schema v6 adds `permission_mode` to threads: an older database's threads read as `default`.

use rusqlite::Connection;
use skyelab_store::{NewThread, Store};

#[test]
fn old_threads_read_default_and_the_mode_persists() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("store.db");
    let store = Store::open(&path).expect("open");
    let p = store
        .create_project("demo", std::path::Path::new("/work/demo"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "t",
            cwd: &p.path,
            account_id: "acme",
            status: "exited",
        })
        .expect("thread");
    assert_eq!(t.permission_mode, "default");
    drop(store);
    // Back to v5, as an older build left it.
    let conn = Connection::open(&path).expect("raw");
    // Undo v14 (PR state), v13 (titles, forks, drafts), v10 (usage readings), v9 (model pick), v8 (project scripts), then v7 (search table, pin/archive/unread/delete columns), then v6.
    conn.execute_batch(
        "ALTER TABLE threads DROP COLUMN pr_state;
         DROP TRIGGER drafts_thread_delete; DROP TABLE drafts; DROP TABLE draft_files;
         ALTER TABLE threads DROP COLUMN title_source; ALTER TABLE threads DROP COLUMN forked_from;
         ALTER TABLE threads DROP COLUMN fork_context;
         DROP TRIGGER usage_reading_insert; DROP TABLE usage_readings;
         ALTER TABLE threads DROP COLUMN model; ALTER TABLE threads DROP COLUMN effort;
         ALTER TABLE projects DROP COLUMN default_model; ALTER TABLE projects DROP COLUMN default_effort;
         DROP TABLE project_scripts;
         DROP TRIGGER event_text_insert; DROP TRIGGER event_text_thread_delete; DROP TABLE event_text;
         ALTER TABLE threads DROP COLUMN pinned_at; ALTER TABLE threads DROP COLUMN archived_at;
         ALTER TABLE threads DROP COLUMN deleted_at; ALTER TABLE threads DROP COLUMN unread;
         ALTER TABLE projects DROP COLUMN removed_at;
         ALTER TABLE threads DROP COLUMN permission_mode; PRAGMA user_version = 5;",
    )
    .expect("downgrade");
    drop(conn);

    let store = Store::open(&path).expect("migrates");
    assert_eq!(
        store
            .thread(t.id)
            .expect("read")
            .expect("kept")
            .permission_mode,
        "default"
    );
    store.set_thread_permission_mode(t.id, "plan").expect("set");
    drop(store);
    let store = Store::open(&path).expect("reopen");
    assert_eq!(
        store.list_threads(p.id).expect("list")[0].permission_mode,
        "plan"
    );
}
