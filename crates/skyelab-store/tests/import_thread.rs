//! A session becomes one thread per project, however often it is imported.

use skyelab_store::{NewThread, Store};
use std::path::Path;

#[test]
fn a_session_is_imported_once_per_project() {
    let mut s = Store::open_in_memory().expect("in memory");
    let p = s
        .create_project("app", Path::new("/work/app"))
        .expect("project");
    let other = s
        .create_project("lib", Path::new("/work/lib"))
        .expect("project");
    let new = |project_id| NewThread {
        project_id,
        title: "Login fix",
        cwd: Path::new("/work/app"),
        account_id: "work",
        status: "stopped",
    };
    let events = [
        ("prompt", "\"hi\"".to_owned()),
        ("agent", "{\"SessionGone\":null}".to_owned()),
    ];

    let t = s
        .import_thread(&new(p.id), "sess-1", 1_700_000_000_000, &events)
        .expect("stored")
        .expect("new session");
    assert_eq!(t.agent_session_id.as_deref(), Some("sess-1"));
    assert_eq!(
        (t.account_id.as_str(), t.updated_at),
        ("work", 1_700_000_000_000)
    );
    let stored = s.events_since(t.id, 0).expect("events");
    assert_eq!(
        stored
            .iter()
            .map(|e| (e.seq, e.kind.as_str()))
            .collect::<Vec<_>>(),
        vec![(1, "prompt"), (2, "agent")]
    );

    assert!(
        s.import_thread(&new(p.id), "sess-1", 1, &events)
            .expect("stored")
            .is_none()
    );
    assert_eq!(s.list_threads(p.id).expect("threads").len(), 1);
    assert!(s.session_ids(p.id).expect("ids").contains("sess-1"));
    // Resuming can give the thread a new session id; the imported one still counts.
    s.set_agent_session_id(t.id, Some("sess-1-resumed"))
        .expect("stored");
    assert!(
        s.import_thread(&new(p.id), "sess-1", 1, &events)
            .expect("stored")
            .is_none()
    );
    let ids = s.session_ids(p.id).expect("ids");
    assert!(
        ids.contains("sess-1") && ids.contains("sess-1-resumed"),
        "{ids:?}"
    );
    // Another project is its own scope.
    assert!(
        s.import_thread(&new(other.id), "sess-1", 1, &events)
            .expect("stored")
            .is_some()
    );
}
