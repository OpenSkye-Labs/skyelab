//! A send that fails while the remote is away must not throw away what the user typed.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_daemon::testing::NODE_NAME;
use std::time::Duration;

#[gpui::test]
fn remote_send_while_reconnecting_keeps_the_prompt(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    click_row(&ws, "app", cx);
    wait_until(cx, "remote threads", |cx| sidebar(&ws, cx)[1].1.len() == 1);
    click_row(&ws, "fix the login bug", cx);
    wait_until(cx, "the tab", |cx| {
        ws.read_with(cx, |ws, _| ws.remote_tabs().len()) == 1
    });
    let view = ws.read_with(cx, |ws, _| {
        ws.remote_tabs().into_iter().next().expect("tab")
    });

    // The account pill stays when another project (or none) is listed.
    wait_until(cx, "account", |cx| {
        view.read_with(cx, |v, cx| v.account(cx).is_some())
    });
    ws.read_with(cx, |ws, _| ws.remote_env().expect("env"))
        .update(cx, |e, cx| e.select_project(-1, cx));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |v, cx| v.account(cx)).as_deref(),
        Some("work")
    );

    // Picking the machine you're on keeps the connection and its tabs.
    switch(&ws, Some(NODE_NAME), cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.remote_tabs().len()), 1);

    remote.stop();
    wait_until(cx, "reconnecting", |cx| {
        state(&ws, cx).starts_with("Reconnecting")
    });
    view.update_in(cx, |v, window, cx| {
        v.submit("a long prompt I typed", window, cx)
    });
    for _ in 0..30 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    let left = view.read_with(cx, |v, cx| v.prompt.read(cx).value().to_string());
    assert_eq!(
        left, "a long prompt I typed",
        "the failed send kept the text"
    );
}
