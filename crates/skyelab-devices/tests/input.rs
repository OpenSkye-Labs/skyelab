//! Agent input through the fake adb fixture (`tests/fixtures/fake-adb.sh`). Unix-only: the fake is a shell script.
#![cfg(unix)]

use skyelab_devices::{Device, DeviceHub, DeviceState, Input, Key, Platform};
use std::fs;
use std::path::{Path, PathBuf};

const FAKE_ADB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake-adb.sh");

/// A copy of the fake adb in its own temp dir (it logs next to itself).
fn fake_adb() -> (tempfile::TempDir, DeviceHub) {
    let dir = tempfile::tempdir().expect("temp dir");
    let adb = dir.path().join("adb");
    fs::copy(FAKE_ADB, &adb).expect("copy fake adb");
    let hub = DeviceHub {
        adb: Some(adb),
        ..DeviceHub::default()
    };
    (dir, hub)
}

fn log(dir: &Path) -> String {
    fs::read_to_string(dir.join("fake-adb.log")).unwrap_or_default()
}

fn emulator(hub: &DeviceHub) -> Device {
    let listing = hub.list_devices();
    let devices = listing.android.expect("fake adb lists");
    assert_eq!(devices.len(), 1, "{devices:?}");
    assert_eq!(devices[0].name, "Fake_Pixel");
    assert_eq!(devices[0].state, DeviceState::Booted);
    devices[0].clone()
}

#[test]
fn input_runs_adb_shell_input_on_the_device() {
    let (dir, hub) = fake_adb();
    let d = emulator(&hub);
    hub.input(&d, &Input::Tap { x: 10, y: 200 }).unwrap();
    hub.input(&d, &Input::Text("hi there; rm -rf".into()))
        .unwrap();
    hub.input(&d, &Input::Key(Key::Home)).unwrap();
    hub.input(&d, &Input::Key(Key::Back)).unwrap();
    let log = log(dir.path());
    for want in [
        "[-s][emulator-5554][shell][input][tap][10][200]",
        "[-s][emulator-5554][shell][input][text]['hi%sthere;%srm%s-rf']",
        "[-s][emulator-5554][shell][input][keyevent][KEYCODE_HOME]",
        "[-s][emulator-5554][shell][input][keyevent][KEYCODE_BACK]",
    ] {
        assert!(log.contains(want), "missing {want} in\n{log}");
    }
}

#[test]
fn screen_changes_after_input() {
    let (dir, hub) = fake_adb();
    let d = emulator(&hub);
    let shot = |name: &str| -> Vec<u8> {
        let png: PathBuf = dir.path().join(name);
        hub.screenshot(&d, &png).unwrap();
        fs::read(png).unwrap()
    };
    let before = shot("a.png");
    hub.input(&d, &Input::Tap { x: 1, y: 1 }).unwrap();
    let after = shot("b.png");
    assert!(before.starts_with(b"\x89PNG") && after.starts_with(b"\x89PNG"));
    assert_ne!(before, after, "the fake flips its screen on input");
}

#[test]
fn ios_input_is_refused() {
    let (dir, hub) = fake_adb();
    let sim = Device {
        id: "ABCD".into(),
        name: "iPhone".into(),
        platform: Platform::Ios,
        os_version: None,
        state: DeviceState::Booted,
        kind: skyelab_devices::DeviceKind::Simulator,
    };
    let err = hub.input(&sim, &Input::Key(Key::Home)).unwrap_err();
    assert!(err.to_string().contains("iOS Simulators"), "{err}");
    assert_eq!(log(dir.path()), "", "nothing ran");
}

/// cargo sets CARGO_MANIFEST_DIR for this test process; a cleared env doesn't pass it on.
#[test]
fn env_is_cleared() {
    let (dir, hub) = fake_adb();
    emulator(&hub);
    let env = fs::read_to_string(dir.path().join("fake-adb.env")).expect("fake wrote its env");
    assert!(!env.contains("CARGO_MANIFEST_DIR="), "{env}");
    assert!(
        env.contains("PATH="),
        "the allowlist still passes PATH: {env}"
    );
}
