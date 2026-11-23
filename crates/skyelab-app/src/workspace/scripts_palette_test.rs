//! Acceptance: the command palette lists "Run script: <name>" for every script of the current project, and the
//! thread header has a Run menu listing them.

use super::worktree_thread_test::{fixture, open_thread, wait_until};
use gpui::TestAppContext;

#[gpui::test]
fn the_palette_and_the_run_menu_list_the_projects_scripts(cx: &mut TestAppContext) {
    let f = fixture();
    let (ws, thread, cx) = open_thread(&f, cx);
    let project = ws.read_with(cx, |ws, _| ws.project.clone().expect("project"));
    {
        let store = f.store.lock().expect("lock");
        store
            .add_project_script(project.id, "Dev server", "npm run dev", false)
            .expect("add");
        store
            .add_project_script(project.id, "Install", "npm i", true)
            .expect("add");
        let other = store
            .create_project("other", &f.project.join("elsewhere"))
            .expect("other");
        store
            .add_project_script(other.id, "Not this one", "true", false)
            .expect("add");
    }
    ws.update(cx, |ws, cx| ws.load_scripts(cx));
    wait_until(cx, "scripts loaded", |cx| {
        ws.read_with(cx, |ws, _| ws.scripts.len() == 2)
    });
    let labels: Vec<String> = ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
            .iter()
            .map(|e| e.label.to_string())
            .collect()
    });
    for want in ["Run script: Dev server", "Run script: Install"] {
        assert!(
            labels.iter().any(|l| l == want),
            "{want} missing from {labels:?}"
        );
    }
    assert!(!labels.iter().any(|l| l.contains("Not this one")));

    let menu = thread.read_with(cx, |t, _| t.scripts.clone());
    menu.update(cx, |m, cx| m.set_open(true, cx));
    wait_until(cx, "the Run menu's list", |cx| {
        menu.read_with(cx, |m, _| m.scripts.len() == 2)
    });
    assert!(menu.read_with(cx, |m, _| m.open));
}
