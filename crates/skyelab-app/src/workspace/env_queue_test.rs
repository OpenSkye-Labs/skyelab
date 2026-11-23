//! A local thread that starts on its own (a delegated child, or a start from another device) never takes the page:
//! while the window shows another machine, the sidebar says a thread is waiting here, and switching back to this
//! machine lists it unread, still without taking the page.

use super::env_harness::*;
use super::thread::ThreadView;
use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn shown(ws: &Entity<Workspace>, view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    ws.read_with(cx, |ws, _| ws.current_thread().as_ref() == Some(view))
}

/// D's stored unread flag, as the view's row carries it.
fn unread(_: &Entity<Workspace>, view: &Entity<ThreadView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |v, _| v.thread.as_ref().is_some_and(|t| t.unread))
}

/// A view that starts in the background: it gets its row (no agent runs here, the folder is unbound), and says so
/// the way a launch does.
fn background_tab(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    ws.update_in(cx, |ws, window, cx| {
        let dir = std::env::temp_dir();
        let (project, row) = {
            let store = lock(&ws.backend.store).expect("store");
            let project = store.create_project("local", &dir).expect("project");
            let row = store
                .create_thread(&skyelab_store::NewThread {
                    project_id: project.id,
                    title: "delegated",
                    cwd: &dir,
                    account_id: "work",
                    status: "running",
                })
                .expect("row");
            (project, row)
        };
        let (config, backend) = (ws.config.clone(), ws.backend.clone());
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        ws.add_thread_tab(view.clone(), false, window, cx);
        view.update(cx, |v, cx| {
            v.thread = Some(row.clone());
            cx.emit(super::thread::ThreadViewEvent::Started(Box::new(row)));
        });
        view
    })
}

#[gpui::test]
fn a_background_thread_waits_while_another_machine_is_shown(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);

    let view = background_tab(&ws, cx);
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.is_remote()),
        "still on the remote machine"
    );
    assert!(!shown(&ws, &view, cx), "the page didn't change");
    let sections = sidebar(&ws, cx);
    let here = sections
        .iter()
        .find(|(title, _)| title == "This Mac")
        .unwrap_or_else(|| panic!("no This Mac section: {sections:?}"));
    assert_eq!(here.1, vec!["1 new thread here".to_owned()]);

    click_row(&ws, "1 new thread here", cx);
    wait_until(cx, "back on this machine", |cx| {
        ws.read_with(cx, |ws, _| !ws.is_remote())
    });
    assert!(!shown(&ws, &view, cx), "still not the page");
    assert!(unread(&ws, &view, cx), "listed unread until opened");
    assert!(
        !sidebar(&ws, cx)
            .iter()
            .any(|(title, _)| title == "This Mac"),
        "nothing waits any more"
    );
}

#[gpui::test]
fn a_background_thread_on_this_machine_keeps_the_page(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let view = background_tab(&ws, cx);
    cx.run_until_parked();
    assert!(
        !shown(&ws, &view, cx),
        "a background start never takes the page"
    );
    assert!(unread(&ws, &view, cx));
}
