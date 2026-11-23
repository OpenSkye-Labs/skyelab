//! A path link reveals a file when the project is a subfolder of its repo (the tab lists repo-root paths).

use super::changes::ChangesView;
use super::changes_model;
use gpui::{AppContext as _, TestAppContext};

#[gpui::test]
fn path_link_reveals_file_in_subfolder_project(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonicalize");
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .expect("git");
    assert!(ok.success());
    let app = root.join("app");
    std::fs::create_dir(&app).expect("mkdir");
    std::fs::write(app.join("notes.txt"), "hi\n").expect("write");
    let changes = changes_model::load(&app, None).expect("load");
    assert_eq!(changes.files.len(), 1);
    let cx = cx.add_empty_window();
    let dir = app.clone();
    let view = cx.update(|window, cx| {
        cx.new(|cx| ChangesView::new(dir, None, "app".into(), None, window, cx))
    });
    cx.run_until_parked();
    view.update(cx, |v, _| (v.changes, v.error) = (Some(changes), None));
    let shown = view.update(cx, |v, cx| v.reveal(&app.join("notes.txt"), cx));
    assert!(shown, "reveal failed for a subfolder project");
    // A same-named file elsewhere in the repo is a different file.
    let other = view.update(cx, |v, cx| v.reveal(&root.join("notes.txt"), cx));
    assert!(!other, "revealed the wrong notes.txt");
}
