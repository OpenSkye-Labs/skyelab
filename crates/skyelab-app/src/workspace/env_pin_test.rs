//! Node pinning holds when the evidence is missing: a pinned name whose server sends no node id, or a settings
//! file that can't be read, is not connected to.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_core::AppSettings;
use skyelab_daemon::testing::NODE_NAME;
use skyelab_protocol::{Hello, Reply, ServerMessage, VERSION};

/// A plaintext server whose hello leaves the node id out, as a 1.0 daemon (or an impostor) would.
pub(super) fn nodeless_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            std::thread::spawn(move || {
                let Ok(mut ws) = tungstenite::accept(stream) else {
                    return;
                };
                let _ = ws.read(); // the hello
                let hello = ServerMessage::Response {
                    id: 0,
                    result: Reply::Hello(Hello {
                        protocol: VERSION,
                        capabilities: vec![],
                        node_id: None,
                    }),
                };
                let _ = ws.send(tungstenite::Message::text(hello.to_json()));
                while ws.read().is_ok() {}
            });
        }
    });
    port
}

fn settle(cx: &mut gpui::VisualTestContext) {
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn a_pinned_name_without_a_node_id_is_refused(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let mut s = AppSettings::default();
    s.trust_environment_node(NODE_NAME, "nSelf");
    s.save(&path).expect("save");
    let mut options = test_options().plaintext();
    options.port = nodeless_server();
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path), options, cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the warning", |cx| {
        state(&ws, cx).contains("different machine")
    });
    settle(cx);
    assert!(state(&ws, cx).contains("different machine"));
    assert!(sidebar(&ws, cx)[0].1.is_empty(), "no projects from it");
    // No node to trust, so no Trust button.
    let button = ws.read_with(cx, |ws, cx| {
        ws.sidebar.read(cx).sections[0].button.is_some()
    });
    assert!(!button, "nothing to trust without a node id");
}

#[gpui::test]
fn an_unpinned_name_without_a_node_id_still_connects(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut options = test_options().plaintext();
    options.port = nodeless_server();
    let (ws, _local, cx) = local_workspace(
        Arc::default(),
        Some(dir.path().join("settings.toml")),
        options,
        cx,
    );
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
}

#[gpui::test]
fn unreadable_settings_refuse_rather_than_trust(cx: &mut TestAppContext) {
    let remote = RemoteSide::new(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "environment_nodes = [[[").expect("broken settings");
    let (ws, _local, cx) = local_workspace(Arc::default(), Some(path), remote.options(), cx);
    switch(&ws, Some(NODE_NAME), cx);
    wait_until(cx, "the refusal", |cx| {
        state(&ws, cx).contains("Can't check which machine")
    });
    settle(cx);
    assert!(state(&ws, cx).contains("Can't check which machine"));
    assert!(
        sidebar(&ws, cx)[0].1.is_empty(),
        "no projects from an unchecked node"
    );
}
