//! Acceptance tests for the device hub. Fake tools are shell scripts that log their argv, so these are Unix-only.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use skyelab_devices::{
    Device, DeviceHub, DeviceKind, DeviceState, Error, Platform, parse_adb_devices, parse_avd_list,
    parse_simctl_list,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(name)).expect("test setup")
}

/// Writes an executable fake tool that logs each call as `[arg1][arg2]…` to `<name>.log`, then runs `body`.
fn fake(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let log = dir.join(format!("{name}.log"));
    let script = format!(
        "#!/bin/sh\n{{ for a in \"$@\"; do printf '[%s]' \"$a\"; done; echo; }} >> '{}'\n{body}\n",
        log.display()
    );
    fs::write(&path, script).expect("test setup");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("test setup");
    path
}

fn log(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(format!("{name}.log"))).unwrap_or_default()
}

fn sim(id: &str) -> Device {
    Device {
        id: id.into(),
        name: "Sim".into(),
        platform: Platform::Ios,
        os_version: None,
        state: DeviceState::Booted,
        kind: DeviceKind::Simulator,
    }
}

fn droid(id: &str, state: DeviceState) -> Device {
    Device {
        id: id.into(),
        name: "Droid".into(),
        platform: Platform::Android,
        os_version: None,
        state,
        kind: DeviceKind::Emulator,
    }
}

fn hub(xcrun: Option<PathBuf>, adb: Option<PathBuf>, emulator: Option<PathBuf>) -> DeviceHub {
    DeviceHub {
        xcrun,
        adb,
        emulator,
    }
}

// 1
#[test]
fn parses_simctl_list() {
    let devices = parse_simctl_list(fixture("simctl_list.json").as_bytes()).expect("test setup");
    assert_eq!(
        devices.len(),
        3,
        "the unavailable device is dropped: {devices:?}"
    );
    let a = devices
        .iter()
        .find(|d| d.id == "00000000-0000-0000-0000-000000000001")
        .expect("test setup");
    assert_eq!(a.name, "Test Phone A");
    assert_eq!(a.platform, Platform::Ios);
    assert_eq!(a.kind, DeviceKind::Simulator);
    assert_eq!(a.os_version.as_deref(), Some("iOS 27.1"));
    assert_eq!(a.state, DeviceState::Shutdown);
    let b = devices
        .iter()
        .find(|d| d.id.ends_with("0002"))
        .expect("test setup");
    assert_eq!(b.state, DeviceState::Booted);
    assert_eq!(b.os_version.as_deref(), Some("iOS 27.0"));
    let tablet = devices
        .iter()
        .find(|d| d.id.ends_with("0003"))
        .expect("test setup");
    assert_eq!(
        tablet.state,
        DeviceState::Offline,
        "transitional states are not usable"
    );
}

// 2
#[test]
fn parses_adb_devices() {
    let devices = parse_adb_devices(&fixture("adb_devices.txt"));
    let by_id = |id: &str| {
        devices
            .iter()
            .find(|d| d.id == id)
            .expect("test setup")
            .clone()
    };
    assert_eq!(devices.len(), 5);
    let emu = by_id("emulator-5554");
    assert_eq!(
        (emu.kind, emu.state),
        (DeviceKind::Emulator, DeviceState::Booted)
    );
    assert_eq!(emu.name, "sdk_gphone64_arm64");
    assert_eq!(emu.platform, Platform::Android);
    let offline = by_id("emulator-5556");
    assert_eq!(
        (offline.kind, offline.state),
        (DeviceKind::Emulator, DeviceState::Offline)
    );
    assert_eq!(
        offline.name, "emulator-5556",
        "no model: falls back to the serial"
    );
    let phone = by_id("TESTSERIAL01");
    assert_eq!(
        (phone.kind, phone.state),
        (DeviceKind::Physical, DeviceState::Booted)
    );
    assert_eq!(phone.name, "Test_Handset");
    assert_eq!(by_id("TESTSERIAL02").state, DeviceState::Offline);
    assert_eq!(by_id("TESTSERIAL03").state, DeviceState::Offline);
}

// 3
#[test]
fn parses_avd_list() {
    assert_eq!(
        parse_avd_list(&fixture("emulator_avds.txt")),
        ["Test_AVD_One", "Test_AVD_Two"]
    );
}

// 4
#[test]
fn a_missing_toolchain_never_hides_the_other_platform() {
    let dir = tempfile::tempdir().expect("test setup");
    let adb = fake(
        dir.path(),
        "adb",
        "echo 'List of devices attached'; echo 'TESTSERIAL01 device model:X'",
    );
    let listing = hub(None, Some(adb), None).list_devices();
    assert!(
        matches!(listing.ios, Err(Error::NotAvailable(Platform::Ios))),
        "{:?}",
        listing.ios
    );
    assert_eq!(listing.android.as_ref().expect("test setup").len(), 1);
    assert_eq!(listing.devices().len(), 1);

    let xcrun = fake(
        dir.path(),
        "xcrun",
        &format!("cat '{FIXTURES}/simctl_list.json'"),
    );
    let listing = hub(Some(xcrun), None, None).list_devices();
    assert!(matches!(
        listing.android,
        Err(Error::NotAvailable(Platform::Android))
    ));
    assert_eq!(listing.ios.as_ref().expect("test setup").len(), 3);
    assert_eq!(listing.devices().len(), 3);
}

// 5 (iOS)
#[test]
fn ios_operations_pass_exact_argv() {
    let dir = tempfile::tempdir().expect("test setup");
    let h = hub(Some(fake(dir.path(), "xcrun", "")), None, None);
    let d = sim("U1");
    assert!(h.boot(&d).expect("test setup").is_none());
    h.shutdown(&d).expect("test setup");
    h.install(&d, Path::new("/apps/My App.app"))
        .expect("test setup");
    h.launch(&d, "com.example.app").expect("test setup");
    h.open_url(&d, "myapp://x?a=1&b=2").expect("test setup");
    h.screenshot(&d, Path::new("/tmp/shot.png"))
        .expect("test setup");
    assert_eq!(
        log(dir.path(), "xcrun"),
        "[simctl][boot][U1]\n[simctl][shutdown][U1]\n[simctl][install][U1][/apps/My App.app]\n\
         [simctl][launch][U1][com.example.app]\n[simctl][openurl][U1][myapp://x?a=1&b=2]\n\
         [simctl][io][U1][screenshot][/tmp/shot.png]\n"
    );
}

// 5 (Android)
#[test]
fn android_operations_pass_exact_argv() {
    let dir = tempfile::tempdir().expect("test setup");
    let adb = fake(
        dir.path(),
        "adb",
        r#"[ "$3" = exec-out ] && printf '\211PNGDATA'; exit 0"#,
    );
    let h = hub(None, Some(adb), None);
    let d = droid("emulator-5554", DeviceState::Booted);
    h.install(&d, Path::new("/apps/app.apk"))
        .expect("test setup");
    h.launch(&d, "com.example/.MainActivity")
        .expect("test setup");
    h.open_url(&d, "https://example.com/?a=1")
        .expect("test setup");
    h.shutdown(&d).expect("test setup");
    let png = dir.path().join("shot.png");
    h.screenshot(&d, &png).expect("test setup");
    assert_eq!(fs::read(&png).expect("test setup"), b"\x89PNGDATA");
    assert_eq!(
        log(dir.path(), "adb"),
        "[-s][emulator-5554][install][-r][/apps/app.apk]\n\
         [-s][emulator-5554][shell][am][start][-n]['com.example/.MainActivity']\n\
         [-s][emulator-5554][shell][am][start][-a][android.intent.action.VIEW][-d]['https://example.com/?a=1']\n\
         [-s][emulator-5554][emu][kill]\n\
         [-s][emulator-5554][exec-out][screencap][-p]\n"
    );
}

// 6
#[test]
fn booting_an_avd_spawns_a_stoppable_emulator() {
    let dir = tempfile::tempdir().expect("test setup");
    let marker = dir.path().join("got-term");
    let emulator = fake(
        dir.path(),
        "emulator",
        &format!(
            "trap 'touch \"{}\"; exit 0' TERM\nwhile :; do sleep 0.1; done",
            marker.display()
        ),
    );
    let h = hub(None, Some(fake(dir.path(), "adb", "")), Some(emulator));
    let mut emu = h
        .boot(&droid("Test_AVD_One", DeviceState::Shutdown))
        .expect("test setup")
        .expect("test setup");
    let start = Instant::now();
    while log(dir.path(), "emulator").is_empty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(log(dir.path(), "emulator"), "[-avd][Test_AVD_One]\n");
    emu.stop().expect("test setup");
    assert!(
        marker.exists(),
        "stop() asks the emulator to quit (SIGTERM) before killing it"
    );
}

// 7
#[test]
fn remote_shell_args_are_quoted_and_local_args_are_not_shelled() {
    let dir = tempfile::tempdir().expect("test setup");
    let h = hub(
        Some(fake(dir.path(), "xcrun", "")),
        Some(fake(dir.path(), "adb", "")),
        None,
    );
    let marker = dir.path().join("pwned");
    let evil = format!("x://a'b;c&d$(touch {})", marker.display());
    h.open_url(&droid("emulator-5554", DeviceState::Booted), &evil)
        .expect("test setup");
    let quoted = format!("'x://a'\\''b;c&d$(touch {})'", marker.display());
    assert!(
        log(dir.path(), "adb").contains(&format!("[-d][{quoted}]")),
        "{}",
        log(dir.path(), "adb")
    );

    h.open_url(&sim("U1"), &evil).expect("test setup");
    assert_eq!(
        log(dir.path(), "xcrun"),
        format!("[simctl][openurl][U1][{evil}]\n")
    );
    assert!(!marker.exists(), "no local shell ran the argument");
}

// 8
#[test]
fn a_failing_tool_returns_its_stderr() {
    let dir = tempfile::tempdir().expect("test setup");
    let h = hub(
        Some(fake(
            dir.path(),
            "xcrun",
            "echo 'Invalid device: U1' >&2; exit 148",
        )),
        None,
        None,
    );
    match h.boot(&sim("U1")) {
        Err(Error::Command { stderr, .. }) => {
            assert!(stderr.contains("Invalid device: U1"), "{stderr}")
        }
        other => panic!("expected Command error, got {other:?}"),
    }
}

// 9
#[test]
fn a_running_avd_is_listed_once() {
    let dir = tempfile::tempdir().expect("test setup");
    let adb = fake(
        dir.path(),
        "adb",
        &format!(
            r#"case "$3" in
  emu) printf 'Test_AVD_One\r\nOK\r\n' ;;
  shell) [ "$5" = sys.boot_completed ] && printf '1\r\n' || printf '16\r\n' ;;
  *) cat '{FIXTURES}/adb_devices.txt' ;;
esac"#
        ),
    );
    let emulator = fake(
        dir.path(),
        "emulator",
        &format!("cat '{FIXTURES}/emulator_avds.txt'"),
    );
    let devices = hub(None, Some(adb), Some(emulator))
        .list_devices()
        .android
        .expect("test setup");
    let running = devices
        .iter()
        .find(|d| d.id == "emulator-5554")
        .expect("test setup");
    assert_eq!(running.name, "Test_AVD_One");
    assert_eq!(running.os_version.as_deref(), Some("16"));
    assert_eq!(
        devices
            .iter()
            .find(|d| d.id == "TESTSERIAL01")
            .expect("test setup")
            .os_version
            .as_deref(),
        Some("16")
    );
    assert_eq!(
        devices
            .iter()
            .find(|d| d.id == "emulator-5556")
            .expect("test setup")
            .os_version,
        None
    );
    assert!(
        !devices.iter().any(|d| d.id == "Test_AVD_One"),
        "running AVD not duplicated: {devices:?}"
    );
    let stopped = devices
        .iter()
        .find(|d| d.id == "Test_AVD_Two")
        .expect("test setup");
    assert_eq!(
        (stopped.state, stopped.kind, stopped.name.as_str()),
        (DeviceState::Shutdown, DeviceKind::Emulator, "Test_AVD_Two")
    );
    assert_eq!(stopped.os_version, None);
    assert_eq!(devices.len(), 6);
}

// 10
#[test]
fn discovery_prefers_the_android_sdk_over_path() {
    let dir = tempfile::tempdir().expect("test setup");
    let bin = dir.path().join("bin");
    let sdk = dir.path().join("sdk");
    let root = dir.path().join("root");
    for d in [
        &bin,
        &sdk.join("platform-tools"),
        &sdk.join("emulator"),
        &root.join("platform-tools"),
    ] {
        fs::create_dir_all(d).expect("test setup");
    }
    fake(&bin, "adb", "");
    fake(&bin, "emulator", "");
    fake(&bin, "xcrun", "");
    let path = std::env::join_paths([&bin]).expect("test setup");

    let h = DeviceHub::discover_with(Some(&path), None, None, None);
    assert_eq!(h.adb, Some(bin.join("adb")));
    assert_eq!(h.emulator, Some(bin.join("emulator")));
    assert_eq!(h.xcrun, Some(bin.join("xcrun")));

    fake(&root.join("platform-tools"), "adb", "");
    let h = DeviceHub::discover_with(Some(&path), None, Some(root.as_os_str()), None);
    assert_eq!(h.adb, Some(root.join("platform-tools/adb")));

    fake(&sdk.join("platform-tools"), "adb", "");
    fake(&sdk.join("emulator"), "emulator", "");
    let h = DeviceHub::discover_with(
        Some(&path),
        Some(sdk.as_os_str()),
        Some(root.as_os_str()),
        None,
    );
    assert_eq!(
        h.adb,
        Some(sdk.join("platform-tools/adb")),
        "ANDROID_HOME first"
    );
    assert_eq!(h.emulator, Some(sdk.join("emulator/emulator")));

    let h = DeviceHub::discover_with(None, None, None, None);
    assert_eq!((h.adb, h.emulator, h.xcrun), (None, None, None));
}

// 13
#[test]
fn fixtures_hold_only_placeholder_udids() {
    for entry in fs::read_dir(FIXTURES).expect("test setup") {
        let text = fs::read_to_string(entry.expect("test setup").path()).expect("test setup");
        for (i, _) in text.match_indices('-') {
            // A UUID is 8-4-4-4-12 hex; check each one found by its first dash.
            let Some(start) = i.checked_sub(8) else {
                continue;
            };
            let Some(cand) = text.get(start..start + 36) else {
                continue;
            };
            let is_uuid = cand.split('-').map(str::len).eq([8, 4, 4, 4, 12])
                && cand.chars().all(|c| c == '-' || c.is_ascii_hexdigit());
            if is_uuid {
                assert!(
                    cand.starts_with("00000000-0000-0000-0000-0000000000"),
                    "real UDID in fixture: {cand}"
                );
            }
        }
    }
}

// 11: lists real devices on this machine. Read-only: it never boots or changes anything.
#[test]
#[ignore = "talks to the real simctl/adb on this machine"]
fn smoke_lists_real_devices() {
    let listing = skyelab_devices::list_devices();
    println!("ios: {:#?}\nandroid: {:#?}", listing.ios, listing.android);
    assert!(
        [&listing.ios, &listing.android]
            .iter()
            .any(|r| r.as_ref().is_ok_and(|d| !d.is_empty())),
        "no platform listed any device"
    );
}
