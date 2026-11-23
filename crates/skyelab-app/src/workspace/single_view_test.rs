//! The center shows one page at a time: selecting a thread switches the page instead of adding a tab, hidden threads
//! keep running, background starts only mark their row unread, and back/forward and the saved view work.
#![cfg(unix)] // the fake agent is a shell script

use super::center::Page;
use super::env_harness;
use super::thread::ThreadView;
use super::threads_test::resume::env_reports;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, test_backend, type_and_send, wait_done,
    wait_until,
};
use super::*;
use crate::settings::Section;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::dock::Panel as _;

fn alive(pid: i64) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .status()
        .expect("kill runs")
        .success()
}

fn current(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Option<Entity<ThreadView>> {
    ws.read_with(cx, |ws, _| ws.current_thread())
}

fn page(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Page {
    ws.read_with(cx, |ws, _| ws.page.clone())
}

/// A started thread with its agent alive between turns, and the agent's pid.
fn running_thread(
    f: &super::threads_test::Fixture,
    ws: &Entity<Workspace>,
    cx: &mut VisualTestContext,
) -> (Entity<ThreadView>, i64, i64) {
    let thread = project_thread(f, ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    wait_until(cx, "the fake's env report", |_| {
        !env_reports(&f.store, id).is_empty()
    });
    let pid = env_reports(&f.store, id)[0]["pid"]
        .as_i64()
        .expect("the fake reports its pid");
    (thread, id, pid)
}

#[gpui::test]
fn the_center_is_one_page_without_a_tab_bar(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = cx.add_window_view(|window, cx| {
        Workspace::new(
            std::env::temp_dir(),
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        )
    });
    ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        assert!(
            dock.panel(ws.center.entity_id().into()).is_some(),
            "the page host is the center's panel"
        );
        assert!(
            !ws.center.read(cx).title_bar(cx),
            "the center draws no tab bar"
        );
        assert_eq!(ws.page, Page::Welcome);
    });
}

#[gpui::test]
fn switching_threads_keeps_the_hidden_one_running(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let (a, a_id, pid) = running_thread(&f, &ws, cx);
    assert_eq!(
        current(&ws, cx).as_ref(),
        Some(&a),
        "a new thread is the page"
    );

    let b = project_thread(&f, &ws, cx);
    assert_eq!(
        current(&ws, cx).as_ref(),
        Some(&b),
        "B replaced A as the page"
    );
    ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        assert!(dock.panel(a.entity_id().into()).is_none(), "no thread tab");
        assert!(dock.panel(b.entity_id().into()).is_none(), "no thread tab");
    });
    cx.run_until_parked();
    assert!(alive(pid), "A's agent runs while B shows");
    assert!(
        a.read_with(cx, |t, _| t.session.is_some()),
        "A keeps its session"
    );

    // Back to A through the sidebar's action: the same view, not a new one.
    cx.dispatch_action(OpenThread { id: a_id });
    cx.run_until_parked();
    assert_eq!(current(&ws, cx).as_ref(), Some(&a));
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.open_threads.len()),
        2,
        "no view made or dropped"
    );

    // Criterion 8: closing through the workspace ends the agent and records `stopped`.
    ws.update_in(cx, |ws, window, cx| ws.close_thread(&a, window, cx));
    drop(a);
    wait_until(cx, "the agent gone and the row stopped", |_| {
        let status = f
            .store
            .lock()
            .expect("lock")
            .thread(a_id)
            .expect("read")
            .expect("row")
            .status;
        !alive(pid) && status == "stopped"
    });
    assert_eq!(page(&ws, cx), Page::Welcome, "closing the page falls back");
}

#[gpui::test]
fn a_background_start_marks_its_row_unread_and_keeps_the_page(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let shown = project_thread(&f, &ws, cx);
    let project = ws.read_with(cx, |ws, _| ws.projects[0].clone());

    let child = ws.update_in(cx, |ws, window, cx| {
        let (config, backend) = (ws.config.clone(), ws.backend.clone());
        let view = cx.new(|cx| ThreadView::new(&project, config, backend, window, cx));
        // As a delegated start does: the prompt is submitted, never typed (the view isn't on screen).
        view.update(cx, |v, cx| v.submit_remote("hi", window, cx))
            .expect("submitted");
        ws.add_thread_tab(view.clone(), false, window, cx);
        view
    });
    wait_done(&child, cx);
    assert_eq!(
        current(&ws, cx).as_ref(),
        Some(&shown),
        "the page didn't change"
    );
    let child_id = child.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    let unread = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.sidebar.read(cx).sections[1]
                .items
                .iter()
                .find(|i| i.id == format!("thread-{child_id}").as_str())
                .map(|i| i.unread)
        })
    };
    wait_until(cx, "the child's row", |cx| unread(&ws, cx).is_some());
    assert_eq!(unread(&ws, cx), Some(true), "the child's row is unread");

    cx.dispatch_action(OpenThread { id: child_id });
    cx.run_until_parked();
    assert_eq!(current(&ws, cx).as_ref(), Some(&child));
    assert_eq!(unread(&ws, cx), Some(false), "opening it marks it read");
}

#[gpui::test]
fn back_and_forward_walk_the_pages(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let a = project_thread(&f, &ws, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    assert_eq!(page(&ws, cx), Page::Settings(Section::Accounts));

    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(current(&ws, cx).as_ref(), Some(&a), "back to the thread");
    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(page(&ws, cx), Page::Welcome, "back to the start");
    cx.dispatch_action(GoForward);
    cx.dispatch_action(GoForward);
    cx.run_until_parked();
    assert_eq!(page(&ws, cx), Page::Settings(Section::Accounts));

    // A closed thread is skipped.
    ws.update_in(cx, |ws, window, cx| ws.close_thread(&a, window, cx));
    drop(a);
    cx.run_until_parked();
    cx.dispatch_action(GoBack);
    cx.run_until_parked();
    assert_eq!(
        page(&ws, cx),
        Page::Welcome,
        "the closed thread was skipped"
    );
}

#[gpui::test]
fn settings_and_remote_are_pages_with_sections(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone().expect("settings exists"));
    assert_eq!(page(&ws, cx), Page::Settings(Section::Accounts));
    ws.read_with(cx, |ws, cx| {
        assert!(
            ws.dock
                .read(cx)
                .panel(settings.entity_id().into())
                .is_none(),
            "settings is not a tab"
        )
    });
    cx.dispatch_action(crate::remote::OpenRemote);
    cx.run_until_parked();
    assert_eq!(page(&ws, cx), Page::Settings(Section::Sharing));
    assert_eq!(settings.read_with(cx, |s, _| s.section), Section::Sharing);
    cx.dispatch_action(OpenSettingsSection {
        section: Section::GitHub,
    });
    cx.run_until_parked();
    assert_eq!(settings.read_with(cx, |s, _| s.section), Section::GitHub);
}

#[gpui::test]
fn the_view_is_saved_and_restored(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let path = f.project_dir.parent().expect("root").join("settings.toml");
    let (ws, cx) = open_workspace(&f, cx);
    let a = project_thread(&f, &ws, cx);
    type_and_send(&a, cx);
    wait_done(&a, cx);
    let id = a.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);

    ws.update_in(cx, |ws, window, cx| {
        ws.restore_layout(Some(path.clone()), None, window, cx)
    });
    cx.dispatch_action(OpenSettings);
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(2));
    wait_until(cx, "the saved view", |_| {
        skyelab_core::AppSettings::load(&path)
            .ok()
            .and_then(|s| s.window)
            .is_some_and(|w| w.view == "settings:accounts")
    });

    // With the thread's view gone (as at a fresh start), the saved view opens it from the store.
    ws.update_in(cx, |ws, window, cx| ws.close_thread(&a, window, cx));
    drop(a);
    let saved = skyelab_core::WindowLayout {
        view: format!("thread:{id}"),
        ..Default::default()
    };
    ws.update_in(cx, |ws, window, cx| {
        ws.restore_layout(None, Some(&saved), window, cx)
    });
    wait_until(cx, "the saved thread as the page", |cx| {
        ws.read_with(cx, |ws, cx| {
            ws.current_thread()
                .and_then(|t| t.read(cx).thread.as_ref().map(|t| t.id))
                == Some(id)
        })
    });
}

#[gpui::test]
fn a_remote_thread_is_the_page_and_leaving_the_machine_drops_it(cx: &mut TestAppContext) {
    use env_harness::{
        FakeTailnet, RemoteSideExt as _, click_row, connect, remote_side, sidebar, switch,
    };
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = env_harness::local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    click_row(&ws, "app", cx);
    env_harness::wait_until(cx, "remote threads", |cx| sidebar(&ws, cx)[1].1.len() == 1);
    click_row(&ws, "fix the login bug", cx);
    env_harness::wait_until(cx, "the remote page", |cx| {
        matches!(page(&ws, cx), Page::RemoteThread(_))
    });
    let view = ws.read_with(cx, |ws, _| ws.remote_tabs()[0].clone());
    ws.read_with(cx, |ws, cx| {
        assert!(
            ws.dock.read(cx).panel(view.entity_id().into()).is_none(),
            "not a tab"
        )
    });
    switch(&ws, None, cx);
    cx.run_until_parked();
    assert_eq!(page(&ws, cx), Page::Welcome);
    assert!(ws.read_with(cx, |ws, _| ws.remote_tabs().is_empty()));
}
