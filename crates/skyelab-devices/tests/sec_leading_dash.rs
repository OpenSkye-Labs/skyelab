#![cfg(unix)]
use skyelab_devices::{Device, DeviceHub, DeviceKind, DeviceState, Platform};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn leading_dash_values_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let tool = dir.path().join("tool");
    std::fs::write(&tool, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    let h = DeviceHub {
        xcrun: Some(tool.clone()),
        adb: Some(tool),
        emulator: None,
    };
    let ios = Device {
        id: "U1".into(),
        name: "S".into(),
        platform: Platform::Ios,
        os_version: None,
        state: DeviceState::Booted,
        kind: DeviceKind::Simulator,
    };
    let droid = Device {
        id: "emulator-5554".into(),
        name: "D".into(),
        platform: Platform::Android,
        os_version: None,
        state: DeviceState::Booted,
        kind: DeviceKind::Emulator,
    };
    assert!(
        h.launch(&ios, "--stdout=/tmp/x").is_err(),
        "DASH_ACCEPTED launch"
    );
    assert!(
        h.install(&droid, Path::new("-g")).is_err(),
        "DASH_ACCEPTED install"
    );
    assert!(
        h.screenshot(&ios, Path::new("--mask=black")).is_err(),
        "DASH_ACCEPTED screenshot"
    );
}
