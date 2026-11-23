//! T3's flat sidebar: every project's threads in one list (pinned first, then newest), the project scope filter,
//! and New thread picking its project.

use super::manage_harness::{Rig, rig};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_store::NewThread as NewRow;

/// A second project, "other", in its own folder (unbound: no account is guessed for it).
fn other_project(r: &Rig, cx: &mut VisualTestContext) -> i64 {
    let dir = r.f.project_dir.parent().expect("parent").join("other");
    std::fs::create_dir_all(&dir).expect("other dir");
    r.ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    cx.run_until_parked();
    r.ws.read_with(cx, |ws, _| {
        ws.projects
            .iter()
            .find(|p| p.name == "other")
            .expect("added")
            .id
    })
}

fn thread_in(r: &Rig, project: i64, title: &str, cx: &mut VisualTestContext) -> i64 {
    let id =
        r.f.store
            .lock()
            .expect("lock")
            .create_thread(&NewRow {
                project_id: project,
                title,
                cwd: &r.f.project_dir,
                account_id: "acme",
                status: "idle",
            })
            .expect("thread")
            .id;
    std::thread::sleep(std::time::Duration::from_millis(3)); // distinct updated_at, so the order is defined
    r.reload(cx);
    id
}

fn projects_of_rows(r: &Rig, cx: &mut VisualTestContext) -> Vec<String> {
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    sidebar.read_with(cx, |s, _| {
        s.sections
            .iter()
            .flat_map(|s| &s.items)
            .map(|i| i.project.as_deref().unwrap_or("-").to_owned())
            .collect()
    })
}

#[gpui::test]
fn threads_of_every_project_are_listed_together_pinned_first(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let other = other_project(&r, cx);
    let a = thread_in(&r, r.project, "a", cx);
    thread_in(&r, other, "b", cx);
    thread_in(&r, r.project, "c", cx);
    assert_eq!(
        r.listed(cx),
        ["c", "b", "a"],
        "newest first, across projects"
    );
    assert_eq!(projects_of_rows(&r, cx), ["demo", "other", "demo"]);

    cx.dispatch_action(manage::ManageThread {
        id: a,
        op: manage::ThreadOp::Pin,
    });
    cx.run_until_parked();
    assert_eq!(r.listed(cx), ["a", "c", "b"], "pinned on top");
}

#[gpui::test]
fn the_scope_filter_narrows_the_list_and_new_thread_uses_it(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let other = other_project(&r, cx);
    thread_in(&r, r.project, "mine", cx);
    thread_in(&r, other, "theirs", cx);

    cx.dispatch_action(ToggleProjectScope { id: other });
    cx.run_until_parked();
    assert_eq!(r.listed(cx), ["theirs"]);

    // One project in scope: New thread starts there, in that project's folder.
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let started_in = r.ws.read_with(cx, |ws, cx| {
        ws.open_threads
            .iter()
            .filter_map(|v| v.upgrade())
            .map(|v| v.read(cx).project_id)
            .collect::<Vec<_>>()
    });
    assert_eq!(started_in, [other]);

    cx.dispatch_action(ShowAllProjects);
    cx.run_until_parked();
    assert_eq!(r.listed(cx), ["theirs", "mine"]);

    // Two projects and no scope: New thread asks which project (the palette), and starts nothing yet.
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    assert!(r.dialog_open(cx), "the project picker is shown");
    assert_eq!(r.ws.read_with(cx, |ws, _| ws.open_threads.len()), 1);
}

#[gpui::test]
fn opening_a_thread_follows_its_project(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let other = other_project(&r, cx);
    let t = thread_in(&r, other, "theirs", cx);
    r.open(t, cx);
    let (project, account) = r.ws.read_with(cx, |ws, _| {
        (ws.project.as_ref().map(|p| p.id), ws.account.clone())
    });
    assert_eq!(project, Some(other));
    assert_eq!(
        account, None,
        "the other folder is unbound: no account is guessed"
    );
}
