//! A listed thread carries whether it is pinned and its agent's last reply (protocol 1.4), so another machine's
//! sidebar can show the same flat list as this one.

use std::path::Path;
use std::sync::{Arc, Mutex};

use skyelab_daemon::{Backend, NotWired, StoreBackend};
use skyelab_store::{NewThread, Store};

#[test]
fn threads_carry_pin_and_preview() {
    let mut store = Store::open_in_memory().expect("store");
    let project = store
        .create_project("app", Path::new("/src/app"))
        .expect("project");
    let thread = |title: &str| {
        store
            .create_thread(&NewThread {
                project_id: project.id,
                title,
                cwd: Path::new("/src/app"),
                account_id: "work",
                status: "exited",
            })
            .expect("thread")
            .id
    };
    let (answered, quiet) = (thread("answered"), thread("quiet"));
    store.set_thread_pinned(answered, true).expect("pin");
    let reply = serde_json::json!({"Assistant": {"content": [{"type": "text", "text": "Fixed **the login** bug."}]}});
    store
        .append_event(answered, "agent", &reply.to_string())
        .expect("reply");
    let backend = StoreBackend::shared(Arc::new(Mutex::new(store)), NotWired, None);
    let threads = backend.list_threads(project.id).expect("threads");
    let of = |id| threads.iter().find(|t| t.id == id).expect("listed");
    assert!(of(answered).pinned);
    assert_eq!(
        of(answered).preview.as_deref(),
        Some("Fixed **the login** bug.")
    );
    assert!(!of(quiet).pinned);
    assert_eq!(of(quiet).preview, None);
}
