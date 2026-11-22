//! The emulator console answers nothing when adb can't read `~/.emulator_console_auth_token` (another `HOME`), so
//! the AVD name comes from a system property instead. Fixtures are recorded from a real emulator. Unix-only: the fake
//! adb is sh.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use skyelab_devices::DeviceHub;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn an_unauthenticated_console_falls_back_to_the_avd_name_property() {
    let dir = tempfile::tempdir().expect("test setup");
    let adb = dir.path().join("adb");
    // argv: -s <serial> emu avd name | -s <serial> shell getprop <prop> | devices -l
    let script = format!(
        r#"#!/bin/sh
case "$3/$5" in
  emu/*) ;;
  shell/ro.boot.qemu.avd_name) cat '{FIXTURES}/getprop_avd_name.txt' ;;
  shell/sys.boot_completed) printf '1\r\n' ;;
  shell/*) printf '16\r\n' ;;
  *) cat '{FIXTURES}/adb_devices_emulator.txt' ;;
esac
"#
    );
    fs::write(&adb, script).expect("test setup");
    fs::set_permissions(&adb, fs::Permissions::from_mode(0o755)).expect("test setup");
    let hub = DeviceHub {
        xcrun: None,
        adb: Some(adb),
        emulator: None,
    };
    let devices = hub.list_devices().android.expect("test setup");
    assert_eq!(devices[0].name, "Pixel_8_API_36");
}
