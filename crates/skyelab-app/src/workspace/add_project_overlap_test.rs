//! A second add while a create is still running must not drop the first.

use super::add_project::Target;
use super::add_project_rig::rig;
use gpui::TestAppContext;

#[gpui::test]
fn a_second_add_does_not_drop_a_pending_create(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx, |_, _| {});
    let (demo, api) = (rig.home.join("code/demo"), rig.home.join("code/api"));
    let ws = rig.ws.clone();
    cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.finish_add(Target::New(demo.clone()), Some("work".into()), window, cx);
            ws.add_project(api.clone(), cx);
        })
    });
    rig.wait_project(&api, cx);
    cx.run_until_parked();
    let projects = rig.projects(cx);
    assert!(projects.contains(&demo), "the created project was dropped: {projects:?}");
}
