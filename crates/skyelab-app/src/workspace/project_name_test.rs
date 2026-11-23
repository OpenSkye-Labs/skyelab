//! One name per project: a thread's header names its project as the sidebar does, not its folder, and follows a
//! rename.

use super::manage_harness::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn the_header_names_the_project(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let rename = |name: &str, cx: &mut gpui::VisualTestContext| {
        r.f.store
            .lock()
            .expect("lock")
            .rename_project(r.project, name)
            .expect("rename");
        r.ws.update(cx, |ws, cx| ws.load_projects(None, cx));
        let ws = r.ws.clone();
        let name = name.to_owned();
        wait_until(cx, "the rename listed", move |cx| {
            ws.read_with(cx, |ws, _| ws.projects.iter().any(|p| p.name == name))
        });
    };
    rename("Website", cx);
    let folder =
        r.f.project_dir
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
    assert_ne!(
        folder, "Website",
        "the folder's name differs from the project's"
    );
    let id = r.thread("Fix it", cx);
    let view = r.open(id, cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.project_name.to_string()),
        "Website"
    );

    rename("Renamed", cx);
    assert_eq!(
        view.read_with(cx, |v, _| v.project_name.to_string()),
        "Renamed",
        "an open thread follows the rename"
    );
}
