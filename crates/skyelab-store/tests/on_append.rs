use std::path::Path;
use std::sync::{Arc, Mutex};

use skyelab_store::{NewThread, Store};

#[test]
fn on_append_sees_each_stored_event_in_order() {
    let mut store = Store::open_in_memory().unwrap();
    let project = store.create_project("demo", Path::new("/repo")).unwrap();
    let thread = store
        .create_thread(&NewThread {
            project_id: project.id,
            title: "t",
            cwd: Path::new("/repo"),
            account_id: "acme",
            status: "running",
        })
        .unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = seen.clone();
    store.on_append(move |e| sink.lock().unwrap().push(e.clone()));
    let a = store.append_event(thread.id, "prompt", "{}").unwrap();
    let b = store.append_event(thread.id, "agent", "{}").unwrap();
    // A failed append (no such thread) reports nothing.
    assert!(store.append_event(9999, "prompt", "{}").is_err());
    assert_eq!(*seen.lock().unwrap(), vec![a, b]);
}
