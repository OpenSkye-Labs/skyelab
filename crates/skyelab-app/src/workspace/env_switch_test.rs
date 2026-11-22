//! Remote mode end to end: another machine's projects, threads and live events in this window, sending and
//! interrupting through its daemon, and back to "This Mac". Local and remote stores never mix.

use super::env_harness::*;
use super::remote_thread::RemoteThreadView;
use super::thread_model::Item;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_daemon::testing::NODE_NAME;

fn texts(view: &Entity<RemoteThreadView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |v, _| {
        v.items
            .iter()
            .map(|i| match i {
                Item::Prompt(t) => format!("> {t}"),
                Item::Text(t) => t.clone(),
                other => format!("{other:?}"),
            })
            .collect()
    })
}

fn only_tab(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<RemoteThreadView> {
    let tabs = ws.read_with(cx, |ws, _| ws.remote_tabs());
    assert_eq!(tabs.len(), 1, "one remote thread tab");
    tabs.into_iter().next().expect("one tab")
}

#[gpui::test]
fn remote_projects_threads_and_live_events_with_a_gap(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    remote.store_texts(remote.thread_id, &["first answer"], false);
    let (ws, local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);

    // Projects come from the remote daemon; there is no local "Add" in remote mode.
    let sections = sidebar(&ws, cx);
    assert_eq!(sections[0], ("Projects".into(), vec!["app".into()]));
    click_row(&ws, "app", cx);
    wait_until(cx, "remote threads", |cx| {
        sidebar(&ws, cx)[1].1 == vec!["fix the login bug".to_owned()]
    });

    click_row(&ws, "fix the login bug", cx);
    wait_until(cx, "the thread tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
    let view = only_tab(&ws, cx);
    wait_until(cx, "stored events", |cx| {
        texts(&view, cx) == ["first answer"]
    });

    // Live: three new events, only the last pushed. The client fills the gap; each shows once, in order.
    remote.store_texts(remote.thread_id, &["two", "three", "four"], true);
    wait_until(cx, "the gap to fill", |cx| texts(&view, cx).len() == 4);
    assert_eq!(texts(&view, cx), ["first answer", "two", "three", "four"]);

    // The header says where it runs and under which account the server reports.
    view.read_with(cx, |v, cx| {
        assert_eq!(v.env_badge(), format!("Remote · {NODE_NAME}"));
        assert_eq!(v.account(cx).as_deref(), Some("work"));
    });
    // The status bar's environment badge.
    let badge = ws.read_with(cx, |ws, cx| ws.env_badge_text(cx));
    assert_eq!(
        badge.as_deref(),
        Some(format!("{NODE_NAME} · Connected").as_str())
    );

    // Never mixed: the local store stayed empty.
    let local = local.lock().expect("local store");
    assert!(local.list_projects().expect("list").is_empty());
}

#[gpui::test]
fn remote_send_start_and_interrupt_reach_the_backend(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    click_row(&ws, "app", cx);
    wait_until(cx, "remote threads", |cx| sidebar(&ws, cx)[1].1.len() == 1);
    click_row(&ws, "fix the login bug", cx);
    wait_until(cx, "the tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
    let view = only_tab(&ws, cx);

    view.update_in(cx, |v, window, cx| v.submit("try again", window, cx));
    let id = remote.thread_id;
    wait_until(cx, "send_message", |_| {
        remote.ran().contains(&Ran::Send(id, "try again".into()))
    });
    view.update_in(cx, |v, window, cx| {
        v.stop(&super::remote_thread::RemoteStop, window, cx)
    });
    wait_until(cx, "interrupt", |_| {
        remote.ran().contains(&Ran::Interrupt(id))
    });

    // New on a remote project: start_thread, then the view follows the new thread's events.
    ws.update_in(cx, |ws, window, cx| {
        ws.new_remote_thread(&env_mode::NewRemoteThread, window, cx)
    });
    let new = ws.read_with(cx, |ws, _| {
        ws.remote_tabs().into_iter().last().expect("a new tab")
    });
    new.update_in(cx, |v, window, cx| v.submit("add tests", window, cx));
    let project = remote.project_id;
    wait_until(cx, "start_thread", |_| {
        remote
            .ran()
            .contains(&Ran::Start(project, "add tests".into()))
    });
    wait_until(cx, "the new thread's prompt", |cx| {
        texts(&new, cx) == ["> add tests"]
    });
    let new_id = new.read_with(cx, |v, _| v.thread_id().expect("started"));
    remote.store_texts(new_id, &["on it"], true);
    wait_until(cx, "its answer", |cx| {
        texts(&new, cx) == ["> add tests", "on it"]
    });
}

#[gpui::test]
fn remote_mode_hides_local_actions_and_switching_back_closes_remote_tabs(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    {
        let local = local.lock().expect("local store");
        local
            .create_project("mine", Path::new("/local/mine"))
            .expect("local project");
    }
    // Reload so the local project shows.
    ws.update(cx, |ws, cx| ws.load_projects(None, cx));
    wait_until(cx, "local project", |cx| {
        sidebar(&ws, cx)[0].1 == vec!["mine".to_owned()]
    });
    connect(&ws, &remote, cx);
    assert!(
        !sidebar(&ws, cx)
            .iter()
            .any(|(_, rows)| rows.contains(&"mine".to_owned())),
        "no local project in remote mode"
    );

    // No local terminal for a remote project.
    ws.update_in(cx, |ws, window, cx| {
        ws.new_terminal(&NewTerminal, window, cx)
    });
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 0);

    click_row(&ws, "app", cx);
    wait_until(cx, "threads", |cx| sidebar(&ws, cx)[1].1.len() == 1);
    click_row(&ws, "fix the login bug", cx);
    wait_until(cx, "the tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });

    switch(&ws, None, cx);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.remote_env().is_none(), "client dropped");
        assert!(ws.remote_tabs().is_empty(), "remote tabs closed");
    });
    assert_eq!(sidebar(&ws, cx)[0].1, vec!["mine".to_owned()]);
    assert_eq!(ws.read_with(cx, |ws, cx| ws.env_badge_text(cx)), None);
}
