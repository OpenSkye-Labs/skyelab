//! Acceptance 7: schema 21's scheduled tasks, and the task id a thread it started carries.

use skyelab_store::scheduled::ScheduledTask;
use skyelab_store::{NewThread, Store};
use std::path::Path;

fn task(project_id: i64) -> ScheduledTask {
    ScheduledTask {
        project_id,
        title: "Nightly triage".into(),
        prompt: "Look at new issues".into(),
        schedule: "daily 09:00".into(),
        worktree: true,
        permission_mode: "default".into(),
        account_id: "acme".into(),
        enabled: true,
        next_run_at: Some(1_000),
        ..Default::default()
    }
}

#[test]
fn tasks_save_list_update_and_delete() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let mut t = task(p.id);
    t.id = store.save_scheduled_task(&t).expect("insert");
    assert!(t.id > 0);
    let listed = store.scheduled_tasks().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "Nightly triage");
    assert!(listed[0].created_at > 0);

    t.prompt = "Look at new issues and label them".into();
    t.bypass_confirmed = true;
    t.permission_mode = "bypass".into();
    t.model = Some("opus".into());
    assert_eq!(store.save_scheduled_task(&t).expect("update"), t.id);
    let read = store.scheduled_task(t.id).expect("read").expect("there");
    assert_eq!(read.prompt, t.prompt);
    assert!(read.bypass_confirmed);
    assert_eq!(read.model.as_deref(), Some("opus"));

    store
        .set_scheduled_task_enabled(t.id, false)
        .expect("pause");
    store
        .record_scheduled_run(t.id, 2_000, "paused", Some("folder isn't bound"))
        .expect("record");
    store
        .set_scheduled_next_run(t.id, Some(9_000))
        .expect("next");
    let read = store.scheduled_task(t.id).expect("read").expect("there");
    assert!(!read.enabled);
    assert_eq!(read.last_run_at, Some(2_000));
    assert_eq!(read.last_status.as_deref(), Some("paused"));
    assert_eq!(read.last_error.as_deref(), Some("folder isn't bound"));
    assert_eq!(read.next_run_at, Some(9_000));
    // An edit leaves the run record alone.
    store.save_scheduled_task(&read).expect("save again");
    let again = store.scheduled_task(t.id).expect("read").expect("there");
    assert_eq!(again.last_status.as_deref(), Some("paused"));

    store.delete_scheduled_task(t.id).expect("delete");
    assert!(store.scheduled_tasks().expect("list").is_empty());
    assert!(store.delete_scheduled_task(t.id).is_err(), "gone already");
}

#[test]
fn a_thread_started_by_a_task_carries_its_id() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let id = store.save_scheduled_task(&task(p.id)).expect("insert");
    assert_eq!(store.last_scheduled_thread(id).expect("none yet"), None);
    let new = |title| NewThread {
        project_id: p.id,
        title,
        cwd: Path::new("/work/demo"),
        account_id: "acme",
        status: "idle",
    };
    let plain = store.create_thread(&new("by hand")).expect("thread");
    assert_eq!(plain.scheduled_task_id, None);
    for title in ["run 1", "run 2"] {
        let t = store.create_thread(&new(title)).expect("thread");
        store.set_thread_scheduled_task(t.id, id).expect("mark");
    }
    let last = store
        .last_scheduled_thread(id)
        .expect("read")
        .expect("a run");
    let last = store.thread(last).expect("read").expect("there");
    assert_eq!(last.title, "run 2");
    assert_eq!(last.scheduled_task_id, Some(id));
    let listed = store.list_threads(p.id).expect("list");
    assert_eq!(
        listed
            .iter()
            .filter(|t| t.scheduled_task_id == Some(id))
            .count(),
        2
    );
    // Deleting the task keeps its threads (and their badge's id; the task is just gone).
    store.delete_scheduled_task(id).expect("delete");
    assert_eq!(store.list_threads(p.id).expect("list").len(), 3);
}

#[test]
fn schema_is_21() {
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("s.db");
    drop(Store::open(&path).expect("open"));
    let conn = rusqlite_version(&path);
    assert_eq!(conn, 21);
}

fn rusqlite_version(path: &Path) -> i64 {
    // The store's own connection type isn't public; read the header field the pragma reports (offset 60).
    let bytes = std::fs::read(path).expect("db file");
    i64::from(u32::from_be_bytes(
        bytes[60..64].try_into().expect("4 bytes"),
    ))
}
