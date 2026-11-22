//! A dropped `Emulator` must not leave a zombie once the emulator exits. Unix-only: the fake emulator is sh, and `ps`
//! reports the zombie state.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{Duration, Instant};

use skyelab_devices::{Device, DeviceHub, DeviceKind, DeviceState, Platform};

/// `ps` state of `pid`: `Z…` for a zombie, empty once it is reaped.
fn state(pid: u32) -> String {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn a_dropped_emulator_is_reaped_when_it_exits() {
    let dir = tempfile::tempdir().expect("test setup");
    let emulator = dir.path().join("emulator");
    fs::write(&emulator, "#!/bin/sh\nsleep 0.2\n").expect("test setup");
    fs::set_permissions(&emulator, fs::Permissions::from_mode(0o755)).expect("test setup");
    let hub = DeviceHub {
        xcrun: None,
        adb: None,
        emulator: Some(emulator),
    };
    let avd = Device {
        id: "Test_AVD_One".into(),
        name: "Test_AVD_One".into(),
        platform: Platform::Android,
        os_version: None,
        state: DeviceState::Shutdown,
        kind: DeviceKind::Emulator,
    };
    let emu = hub.boot(&avd).expect("test setup").expect("an AVD boot returns its process");
    let pid = emu.pid();
    drop(emu);
    let start = Instant::now();
    while !state(pid).is_empty() && start.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(state(pid), "", "pid {pid} was not reaped");
}
