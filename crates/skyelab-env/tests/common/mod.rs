//! A store with more than the harness's one row: two projects, a pinned thread with a reply, a settled one.

use std::path::Path;

use skyelab_daemon::testing::Env;
use skyelab_store::NewThread;

/// The harness env plus a second project with two threads, served.
pub fn env() -> Env {
    let mut env = Env::new();
    {
        let mut store = env.store.lock().expect("store lock");
        let web = store
            .create_project("web", Path::new("/src/web"))
            .expect("project");
        let thread = |title: &'static str| NewThread {
            project_id: web.id,
            title,
            cwd: Path::new("/src/web"),
            account_id: "work",
            status: "exited",
        };
        let pinned = store.create_thread(&thread("pinned")).expect("thread").id;
        let settled = store.create_thread(&thread("settled")).expect("thread").id;
        store.set_thread_pinned(pinned, true).expect("pin");
        store.set_thread_settled(settled, true).expect("settle");
        let reply =
            serde_json::json!({"Assistant": {"content": [{"type": "text", "text": "Done."}]}});
        store
            .append_event(pinned, "agent", &reply.to_string())
            .expect("reply");
    }
    env.serve();
    env
}
