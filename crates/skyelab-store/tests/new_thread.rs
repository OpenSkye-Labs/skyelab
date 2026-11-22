use skyelab_store::{NewThread, Store};
use std::path::Path;

#[test]
fn create_thread_takes_named_fields_and_records_the_account() {
    let store = Store::open_in_memory().expect("open");
    let p = store
        .create_project("demo", Path::new("/work/demo"))
        .expect("project");
    let t = store
        .create_thread(&NewThread {
            project_id: p.id,
            title: "Fix the build",
            cwd: Path::new("/work/demo/api"),
            account_id: "acme",
            status: "starting",
        })
        .expect("thread");
    let stored = store.thread(t.id).expect("read").expect("exists");
    assert_eq!(stored.title, "Fix the build");
    assert_eq!(stored.cwd, Path::new("/work/demo/api"));
    assert_eq!(stored.account_id, "acme");
    assert_eq!(stored.status, "starting");
}
