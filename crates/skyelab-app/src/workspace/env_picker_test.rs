//! The environment picker: which tailnet devices it offers, adding one by name, and recents in settings.toml.

use super::env_harness::*;
use super::*;
use gpui::TestAppContext;
use skyelab_core::AppSettings;

fn tailnet() -> FakeTailnet {
    FakeTailnet {
        peers: vec![
            node("nLaptop", "laptop.example.ts.net", ME, true, &[]),
            node("nOff", "offline.example.ts.net", ME, false, &[]),
            node(
                "nTagged",
                "server.example.ts.net",
                ME,
                true,
                &["tag:server"],
            ),
            node("nOther", "friend.example.ts.net", 99, true, &[]),
            node("nSelf", "desktop.example.ts.net", ME, true, &[]),
        ],
        ..FakeTailnet::default()
    }
}

#[gpui::test]
fn picker_offers_this_mac_and_only_your_online_devices(cx: &mut TestAppContext) {
    let remote = RemoteSide::new(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::new(tailnet()), None, remote.options(), cx);
    cx.dispatch_action(env_picker::ToggleEnvironments);
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    wait_until(cx, "peers", |cx| {
        picker.read_with(cx, |p, _| !p.peers.is_empty())
    });
    picker.read_with(cx, |p, _| {
        assert!(p.open);
        assert_eq!(p.entries(), ["This Mac", "laptop.example.ts.net"]);
    });

    // Add by MagicDNS name: the typed name becomes the remote environment, and it connects.
    picker.update_in(cx, |p, window, cx| p.set_typed("127.0.0.1", window, cx));
    cx.dispatch_action(env_picker::ConnectTyped);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
    let host = ws.read_with(cx, |ws, cx| {
        ws.remote_env().map(|e| e.read(cx).host.clone())
    });
    assert_eq!(host.as_deref(), Some("127.0.0.1"));
    picker.read_with(cx, |p, _| {
        assert_eq!(p.current.as_deref(), Some("127.0.0.1"))
    });
}

#[gpui::test]
fn picker_remembers_recent_environments_and_keeps_other_settings(cx: &mut TestAppContext) {
    let remote = RemoteSide::new(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    AppSettings {
        share_on_tailnet: true,
        ..AppSettings::default()
    }
    .save(&path)
    .expect("save");
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);

    switch(&ws, Some("127.0.0.1"), cx);
    wait_until(cx, "saved", |_| {
        AppSettings::load(&path).expect("load").recent_environments == ["127.0.0.1"]
    });
    switch(&ws, None, cx);
    switch(&ws, Some("laptop.example.ts.net"), cx);
    switch(&ws, Some("127.0.0.1"), cx);
    wait_until(cx, "reordered", |_| {
        AppSettings::load(&path).expect("load").recent_environments
            == ["127.0.0.1", "laptop.example.ts.net"]
    });
    assert!(
        AppSettings::load(&path).expect("load").share_on_tailnet,
        "kept"
    );
    // Recents show in the picker.
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    picker.read_with(cx, |p, _| {
        assert!(p.entries().contains(&"laptop.example.ts.net".to_owned()))
    });
}
