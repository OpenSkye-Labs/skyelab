#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use skyelab_devices::DeviceHub;

fn tool(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn a_broken_emulator_binary_does_not_hide_adb_devices() {
    let dir = tempfile::tempdir().unwrap();
    let adb = tool(
        dir.path(),
        "adb",
        r#"case "$3" in
  shell) printf '16\r\n' ;;
  *) printf 'List of devices attached\nTESTSERIAL01           device usb:1-1 product:p model:Pixel_9 device:d transport_id:1\n' ;;
esac"#,
    );
    let emulator = tool(
        dir.path(),
        "emulator",
        "echo 'PANIC: Missing emulator engine' >&2; exit 1",
    );
    let hub = DeviceHub {
        xcrun: None,
        adb: Some(adb),
        emulator: Some(emulator),
    };
    let android = hub.list_devices().android;
    let devices =
        android.unwrap_or_else(|e| panic!("BUG adb devices hidden by emulator failure: {e}"));
    assert!(
        devices.iter().any(|d| d.id == "TESTSERIAL01"),
        "{devices:?}"
    );
}
