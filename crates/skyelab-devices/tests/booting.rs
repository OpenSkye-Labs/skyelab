//! An emulator adb lists as `device` is usable only once `sys.boot_completed` is 1. Unix-only: the fake adb is sh.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use skyelab_devices::{DeviceHub, DeviceState};

#[test]
fn booting_emulator_is_not_listed_as_booted() {
    let dir = tempfile::tempdir().expect("test setup");
    let adb = dir.path().join("adb");
    // argv: -s <serial> shell getprop <prop>
    let script = r#"#!/bin/sh
case "$3/$5" in
  shell/sys.boot_completed) [ "$2" = emulator-5554 ] && printf '1\r\n' || printf '\r\n' ;;
  shell/*) printf '16\r\n' ;;
  emu/*) printf 'KO: no\r\n' ;;
  *) printf 'List of devices attached\nemulator-5554 device model:a\nemulator-5556 device model:b\nTESTSERIAL01 device usb:1 model:p\n' ;;
esac
"#;
    fs::write(&adb, script).expect("test setup");
    fs::set_permissions(&adb, fs::Permissions::from_mode(0o755)).expect("test setup");
    let hub = DeviceHub {
        xcrun: None,
        adb: Some(adb),
        emulator: None,
    };
    let devices = hub.list_devices().android.expect("test setup");
    let state = |id: &str| {
        devices
            .iter()
            .find(|d| d.id == id)
            .expect("test setup")
            .state
    };
    assert_eq!(state("emulator-5554"), DeviceState::Booted);
    assert_eq!(state("emulator-5556"), DeviceState::Booting);
    assert_eq!(state("TESTSERIAL01"), DeviceState::Booted);
}

#[test]
fn booting_simulator_is_booting() {
    let json = br#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-27-0":[
        {"udid":"00000000-0000-0000-0000-000000000009","name":"S","state":"Booting","isAvailable":true}]}}"#;
    let devices = skyelab_devices::parse_simctl_list(json).expect("test setup");
    assert_eq!(devices[0].state, DeviceState::Booting);
}
