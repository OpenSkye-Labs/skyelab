//! A remote start that arrives while the workspace is still loading its projects.

use super::worktree_thread_test::{fixture, wait_until};
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn remote_start_before_projects_load_waits_for_them(cx: &mut TestAppContext) {
    let f = fixture();
    let project_id = f
        .store
        .lock()
        .expect("lock")
        .create_project("demo", &f.project)
        .expect("project")
        .id;
    cx.executor().allow_parking(); // the fake agent is a real process
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake-claude.sh"),
    };
    let (reply, answer) = flume::bounded(1);
    let (config, folder) = (f.config.clone(), f.project.clone());
    let (_ws, cx) = cx.add_window_view(|window, cx| {
        let mut ws = Workspace::new(folder, None, config, None, backend, window, cx);
        assert!(ws.project_ids().is_empty());
        ws.remote_start(project_id, "hello", reply, window, cx);
        ws
    });
    wait_until(cx, "the remote start's answer", |_| !answer.is_empty());
    let thread_id = answer
        .try_recv()
        .expect("answered")
        .expect("started once the projects loaded");
    let row = f
        .store
        .lock()
        .expect("lock")
        .thread(thread_id)
        .expect("read");
    assert_eq!(row.expect("row").project_id, project_id);
}
