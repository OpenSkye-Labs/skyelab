//! Settings → Machines: adding a machine by name connects through the same path as the menu, and Forget drops a
//! recent name from settings.toml while keeping everything else there.

use super::env_harness::*;
use super::env_picker::{ConnectTyped, ForgetEnvironment};
use super::*;
use crate::settings::Section;
use gpui::TestAppContext;
use skyelab_core::AppSettings;
use skyelab_daemon::testing::NODE_NAME;

#[gpui::test]
fn the_machines_section_connects_a_typed_name_and_forgets_a_recent(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    AppSettings {
        share_on_tailnet: true,
        recent_environments: vec!["laptop.example.ts.net".into()],
        ..AppSettings::default()
    }
    .save(&path)
    .expect("save");
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);
    cx.dispatch_action(OpenSettingsSection {
        section: Section::Machines,
    });
    cx.run_until_parked();
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone().expect("settings"));
    settings.read_with(cx, |s, _| {
        assert_eq!(s.section, Section::Machines);
        assert!(s.machines.is_some(), "the section has its page");
    });
    assert!(
        cx.debug_bounds("machines-add").is_some(),
        "the add row is drawn"
    );

    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    picker.update_in(cx, |p, window, cx| p.set_typed(NODE_NAME, window, cx));
    cx.dispatch_action(ConnectTyped);
    wait_until(cx, "connected", |cx| state(&ws, cx) == "Connected");
    wait_until(cx, "remembered", |_| {
        AppSettings::load(&path).expect("load").recent_environments
            == [NODE_NAME, "laptop.example.ts.net"]
    });

    cx.dispatch_action(ForgetEnvironment {
        host: "Laptop.example.ts.net".into(),
    });
    wait_until(cx, "forgotten", |_| {
        AppSettings::load(&path).expect("load").recent_environments == [NODE_NAME]
    });
    let saved = AppSettings::load(&path).expect("load");
    assert!(saved.share_on_tailnet, "other settings kept");
    wait_until(cx, "the list follows", |cx| {
        picker.read_with(cx, |p, _| p.recents == [NODE_NAME])
    });
    assert_eq!(
        state(&ws, cx),
        "Connected",
        "forgetting another name keeps the connection"
    );
}

#[gpui::test]
fn an_ip_recent_stays_hidden_after_a_save(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    AppSettings {
        recent_environments: vec!["100.64.0.9".into()],
        ..AppSettings::default()
    }
    .save(&path)
    .expect("save");
    let (ws, _local, cx) =
        local_workspace(Arc::default(), Some(path.clone()), remote.options(), cx);
    let picker = ws.read_with(cx, |ws, _| ws.env_picker().expect("picker"));
    picker.update_in(cx, |p, window, cx| p.set_typed(NODE_NAME, window, cx));
    cx.dispatch_action(ConnectTyped);
    wait_until(cx, "remembered", |_| {
        AppSettings::load(&path).expect("load").recent_environments[0] == NODE_NAME
    });
    // A tailnet IP from an older file is never offered: connecting to it would be refused.
    wait_until(cx, "the list follows", |cx| {
        picker.read_with(cx, |p, _| p.recents == [NODE_NAME])
    });
}
