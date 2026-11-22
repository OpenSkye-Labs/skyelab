//! Regression tests for review findings F5 and F6. Unix-only: fake tools are shell scripts.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use skyelab_devices::{Device, DeviceHub, DeviceKind, DeviceState, Error, Platform};

fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn a_ko_console_reply_does_not_rename_the_emulator() {
    let dir = tempfile::tempdir().unwrap();
    let adb = tool(
        dir.path(),
        "adb",
        r#"case "$3" in
  emu) printf 'KO: unknown command\r\n' ;;
  shell) printf '16\r\n' ;;
  *) printf 'List of devices attached\nemulator-5554 device model:sdk_phone\n' ;;
esac"#,
    );
    let hub = DeviceHub {
        xcrun: None,
        adb: Some(adb),
        emulator: None,
    };
    let devices = hub.list_devices().android.unwrap();
    assert_eq!(devices[0].name, "sdk_phone");
}

#[test]
fn a_non_png_screencap_is_an_error_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let adb = tool(dir.path(), "adb", "printf 'Error: no display'");
    let hub = DeviceHub {
        xcrun: None,
        adb: Some(adb),
        emulator: None,
    };
    let device = Device {
        id: "emulator-5554".into(),
        name: "D".into(),
        platform: Platform::Android,
        os_version: None,
        state: DeviceState::Booted,
        kind: DeviceKind::Emulator,
    };
    let png = dir.path().join("shot.png");
    match hub.screenshot(&device, &png) {
        Err(Error::Command { stderr, .. }) => assert!(stderr.contains("no display"), "{stderr}"),
        other => panic!("expected Command error, got {other:?}"),
    }
    assert!(!png.exists());
}
