//! This machine's sidebar follows its own shell stream: a thread another writer (a remote call, an agent, another
//! window) adds to the store shows, and one deleted there goes, without a reload.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn rows_written_elsewhere_show_without_a_reload(cx: &mut TestAppContext) {
    setup(cx);
    let store: SharedStore = Arc::new(Mutex::new(Store::open_in_memory().expect("store")));
    // The app's own backend, as main.rs sets it: the stream follows only that one.
    cx.update(|cx| {
        cx.set_global(LocalBackend(Arc::new(skyelab_daemon::StoreBackend::shared(
            store.clone(),
            skyelab_daemon::NotWired,
            None,
        ))))
    });
    let backend = Backend {
        store: store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (ws, cx) = cx.add_window_view(|window, cx| {
        Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            backend,
            window,
            cx,
        )
    });
    let project = store
        .lock()
        .expect("store")
        .create_project("app", Path::new("/src/app"))
        .expect("project")
        .id;
    wait_until(cx, "the project", |cx| {
        ws.read_with(cx, |ws, _| ws.project_ids() == [project])
    });
    let title = |id: i64, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, _| {
            ws.threads.iter().find(|t| t.id == id).map(|t| t.title.clone())
        })
    };
    let id = store
        .lock()
        .expect("store")
        .create_thread(&skyelab_store::NewThread {
            project_id: project,
            title: "written elsewhere",
            cwd: Path::new("/src/app"),
            account_id: "work",
            status: "idle",
        })
        .expect("thread")
        .id;
    wait_until(cx, "the new row", |cx| title(id, cx).is_some());
    store.lock().expect("store").delete_thread(id).expect("delete");
    wait_until(cx, "the row gone", |cx| title(id, cx).is_none());
}
