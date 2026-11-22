#![cfg(unix)]
use skyelab_devices::DeviceHub;
use std::os::unix::fs::PermissionsExt;

#[test]
fn relative_path_entries_are_ignored() {
    let crate_dir = env!("CARGO_MANIFEST_DIR");
    let dir = tempfile::tempdir_in(crate_dir).unwrap();
    let tool = dir.path().join("xcrun");
    std::fs::write(&tool, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_current_dir(crate_dir).unwrap();
    let rel = dir.path().file_name().unwrap();
    let h = DeviceHub::discover_with(Some(rel), None, None);
    assert!(
        h.xcrun.as_ref().is_none_or(|p| p.is_absolute()),
        "RELATIVE_TOOL_FOUND {:?}",
        h.xcrun
    );
}
