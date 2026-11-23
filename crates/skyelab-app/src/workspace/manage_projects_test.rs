//! Acceptance (package D): rename and remove projects; removing never touches disk, asks first while agents run,
//! and adding the folder again brings the project back.

use super::manage::{ManageProject, ProjectOp};
use super::manage_harness::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn rename_a_project(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    cx.dispatch_action(ManageProject {
        id: r.project,
        op: ProjectOp::Rename,
    });
    cx.run_until_parked();
    assert!(r.dialog_open(cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    });
    cx.simulate_input("Renamed");
    cx.simulate_keystrokes("enter");
    let ws = r.ws.clone();
    wait_until(cx, "renamed", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.projects.first().is_some_and(|p| p.name == "Renamed")
        })
    });
}

#[gpui::test]
fn remove_keeps_files_and_threads_and_readding_restores(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let t = r.thread("kept", cx);
    std::fs::write(r.f.project_dir.join("file.txt"), "x").expect("write");
    cx.dispatch_action(ManageProject {
        id: r.project,
        op: ProjectOp::Remove,
    });
    cx.run_until_parked();
    let ws = r.ws.clone();
    wait_until(cx, "removed", |cx| {
        ws.read_with(cx, |ws, _| ws.projects.is_empty())
    });
    assert!(!r.dialog_open(cx), "no running agent: nothing to ask");
    assert!(ws.read_with(cx, |ws, _| ws.project.is_none()));
    assert!(r.f.project_dir.join("file.txt").exists(), "disk untouched");
    assert!(r.row(t).is_some(), "threads and their audit stay");

    let dir = r.f.project_dir.clone();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "back", |cx| {
        ws.read_with(cx, |ws, _| {
            ws.project.as_ref().map(|p| p.id) == Some(r.project)
        })
    });
    wait_until(cx, "its thread back", |cx| r.listed(cx) == ["kept"]);
}

#[gpui::test]
fn a_project_with_a_live_agent_asks_first(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let t = r.thread("running", cx);
    let view = r.open(t, cx);
    view.update(cx, |v, _| v.starting = true);
    cx.dispatch_action(ManageProject {
        id: r.project,
        op: ProjectOp::Remove,
    });
    cx.run_until_parked();
    assert!(r.dialog_open(cx), "asks before stopping agents");
    let ws = r.ws.clone();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.projects.len()),
        1,
        "nothing removed yet"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.pending_removal),
        Some(r.project)
    );

    view.update(cx, |v, _| v.starting = false);
    drop(view);
    let id = r.project;
    ws.update_in(cx, |ws, window, cx| ws.remove_project_now(id, window, cx));
    wait_until(cx, "removed", |cx| {
        ws.read_with(cx, |ws, _| ws.projects.is_empty())
    });
    assert!(
        ws.read_with(cx, |ws, cx| ws.open_view(t, cx).is_none()),
        "its tab closed"
    );
}
