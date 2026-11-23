//! After a reconnect nothing reaches the other machine until its identity is approved, and a refusal or identity
//! failure clears whatever the window still showed from it.

use super::env_harness::*;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn tabs(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    ws.read_with(cx, |ws, _| ws.remote_tabs().len())
}

/// Connected to `remote`, its project selected and its thread open in a tab.
fn open_thread(ws: &Entity<Workspace>, remote: &RemoteSide, cx: &mut VisualTestContext) {
    connect(ws, remote, cx);
    click_row(ws, "app", cx);
    wait_until(cx, "remote threads", |cx| !sidebar(ws, cx)[1].1.is_empty());
    click_row(ws, "fix the login bug", cx);
    wait_until(cx, "the thread tab", |cx| tabs(ws, cx) == 1);
}

#[gpui::test]
fn a_reconnect_to_another_node_asks_the_remote_nothing(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let (ws, _local, cx) = local_workspace(
        remote.tailnet.clone(),
        Some(dir.path().join("settings.toml")),
        remote.options(),
        cx,
    );
    open_thread(&ws, &remote, cx);
    wait_until(cx, "the tab's watch", |_| {
        remote.asked().contains(&"thread_events_since")
    });

    // The name comes back from another node.
    remote.stop();
    *remote.tailnet.self_id.lock().expect("self id") = Some("nImpostor".into());
    remote.asked_log.lock().expect("asked lock").clear();
    remote.serve();
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    settle(cx);
    assert_eq!(remote.asked(), Vec::<&str>::new(), "nothing asked of it");
}

#[gpui::test]
fn a_refusal_after_a_trusted_connection_clears_the_window(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(remote.tailnet.clone(), None, remote.options(), cx);
    open_thread(&ws, &remote, cx);

    remote.stop();
    remote.tailnet.set_peer(Peer::Tagged);
    remote.serve();
    wait_until(cx, "the refusal", |cx| {
        state(&ws, cx) == "That machine didn't accept this device"
    });
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects left");
    assert!(sidebar(&ws, cx)[1].1.is_empty(), "no threads left");
    assert_eq!(tabs(&ws, cx), 0, "its tabs closed");
}

#[gpui::test]
fn a_failed_own_check_after_a_trusted_connection_clears_the_window(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let refuse = Arc::new(AtomicBool::new(false));
    let mut options = remote.options();
    let flag = refuse.clone();
    options.verify_peer = Some(skyelab_client::PeerVerifier::new(move |_| {
        if flag.load(Ordering::SeqCst) {
            Err("a tagged node".into())
        } else {
            Ok("nSelf".into())
        }
    }));
    let (ws, _local, cx) = local_workspace(remote.tailnet.clone(), None, options, cx);
    open_thread(&ws, &remote, cx);

    refuse.store(true, Ordering::SeqCst);
    remote.stop();
    remote.serve();
    wait_until(cx, "the refusal", |cx| {
        state(&ws, cx).starts_with("Not connected: your Tailscale")
    });
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects left");
    assert_eq!(tabs(&ws, cx), 0, "its tabs closed");
}
