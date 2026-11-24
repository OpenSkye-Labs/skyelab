//! A clone URL's repo name must be one plain path component on every platform.
use skyelab_core::add_project::clone_name;

#[test]
fn clone_name_refuses_backslash_and_drive_names() {
    for bad in [
        "https://example.com/a\\..\\..\\x",
        "https://example.com/acme/C:",
        "https://example.com/acme/x\\y.git",
    ] {
        assert!(clone_name(bad).is_err(), "{bad} must be refused");
    }
}
