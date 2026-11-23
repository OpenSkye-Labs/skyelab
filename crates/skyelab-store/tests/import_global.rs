//! One agent session continues in one thread across every project (nested projects can both list it), and a session
//! another thread is running can't be resumed a second time.

use skyelab_store::{NewThread, Store};
use std::path::Path;

fn new(project_id: i64) -> NewThread<'static> {
    NewThread {
        project_id,
        title: "Login fix",
        cwd: Path::new("/work/app/sub"),
        account_id: "work",
        status: "stopped",
    }
}

#[test]
fn import_is_refused_across_projects() {
    let mut s = Store::open_in_memory().expect("in memory");
    let outer = s
        .create_project("app", Path::new("/work/app"))
        .expect("project");
    let inner = s
        .create_project("sub", Path::new("/work/app/sub"))
        .expect("project");
    let events = [("prompt", "\"hi\"".to_owned(), 5)];
    assert!(
        s.import_thread(&new(outer.id), "sess-1", 9, &events)
            .expect("stored")
            .is_some()
    );
    assert!(
        s.import_thread(&new(inner.id), "sess-1", 9, &events)
            .expect("stored")
            .is_none(),
        "the nested project can't import it again"
    );
    assert!(s.session_ids().expect("ids").contains("sess-1"));
}

#[test]
fn imported_events_keep_their_own_times() {
    let mut s = Store::open_in_memory().expect("in memory");
    let p = s
        .create_project("app", Path::new("/work/app"))
        .expect("project");
    let events = [
        ("prompt", "\"hi\"".to_owned(), 1_000),
        ("agent", "{\"SessionGone\":null}".to_owned(), 2_000),
    ];
    let t = s
        .import_thread(&new(p.id), "sess-1", 9_000, &events)
        .expect("stored")
        .expect("new");
    let times: Vec<i64> = s
        .events_since(t.id, 0)
        .expect("events")
        .iter()
        .map(|e| e.created_at)
        .collect();
    assert_eq!(times, vec![1_000, 2_000]);
    assert_eq!((t.created_at, t.updated_at), (1_000, 9_000));
}

#[test]
fn session_busy_elsewhere() {
    let mut s = Store::open_in_memory().expect("in memory");
    let p = s
        .create_project("app", Path::new("/work/app"))
        .expect("project");
    let events = [("prompt", "\"hi\"".to_owned(), 1)];
    let a = s
        .import_thread(&new(p.id), "sess-1", 1, &events)
        .expect("stored")
        .expect("new");
    // A second thread on the same session (started here before the import, say).
    let b = s
        .import_thread(&new(p.id), "sess-2", 1, &events)
        .expect("stored")
        .expect("new");
    s.set_agent_session_id(b.id, Some("sess-1"))
        .expect("stored");
    assert!(!s.session_busy_elsewhere(a.id, "sess-1").expect("read"));
    for status in ["starting", "running"] {
        s.set_thread_status(b.id, status).expect("stored");
        assert!(
            s.session_busy_elsewhere(a.id, "sess-1").expect("read"),
            "{status}"
        );
    }
    assert!(
        !s.session_busy_elsewhere(b.id, "sess-1").expect("read"),
        "a thread doesn't block itself"
    );
    // A crashed run leaves `idle` on the row with no agent behind it, so idle doesn't count.
    s.set_thread_status(b.id, "idle").expect("stored");
    assert!(!s.session_busy_elsewhere(a.id, "sess-1").expect("read"));
    s.set_thread_status(b.id, "exited").expect("stored");
    assert!(!s.session_busy_elsewhere(a.id, "sess-1").expect("read"));
}
