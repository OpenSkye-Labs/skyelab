//! With no SDK env vars (a Dock-launched app), the SDK's default install dir under the user dir is searched.

use std::fs;
use std::path::Path;

use skyelab_devices::DeviceHub;

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().expect("has a parent")).expect("test setup");
    fs::write(path, "").expect("test setup");
}

#[test]
fn default_sdk_dir_is_searched_when_no_sdk_var_is_set() {
    let home = tempfile::tempdir().expect("test setup");
    let sdk = if cfg!(target_os = "macos") {
        home.path().join("Library/Android/sdk")
    } else if cfg!(windows) {
        home.path().join("Android").join("Sdk")
    } else {
        home.path().join("Android/Sdk")
    };
    let exe = std::env::consts::EXE_SUFFIX;
    let adb = sdk.join("platform-tools").join(format!("adb{exe}"));
    let emulator = sdk.join("emulator").join(format!("emulator{exe}"));
    touch(&adb);
    touch(&emulator);

    let h = DeviceHub::discover_with(None, None, None, Some(home.path().as_os_str()));
    assert_eq!(h.adb.as_deref(), Some(adb.as_path()));
    assert_eq!(h.emulator.as_deref(), Some(emulator.as_path()));

    let empty = std::ffi::OsStr::new("");
    let h = DeviceHub::discover_with(
        None,
        Some(empty),
        Some(empty),
        Some(home.path().as_os_str()),
    );
    assert_eq!(
        h.adb.as_deref(),
        Some(adb.as_path()),
        "an empty SDK var counts as unset"
    );

    let other = tempfile::tempdir().expect("test setup");
    let h = DeviceHub::discover_with(
        None,
        Some(other.path().as_os_str()),
        None,
        Some(home.path().as_os_str()),
    );
    assert_eq!(
        h.adb, None,
        "ANDROID_HOME set: the default dir is not searched"
    );
    let h = DeviceHub::discover_with(
        None,
        None,
        Some(other.path().as_os_str()),
        Some(home.path().as_os_str()),
    );
    assert_eq!(
        h.adb, None,
        "ANDROID_SDK_ROOT set: the default dir is not searched"
    );
}
