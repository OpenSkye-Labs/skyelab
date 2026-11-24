use skyelab_core::update_install::{Install, detect};
use std::path::Path;

#[test]
fn a_leaked_appimage_env_is_not_this_install() {
    let other = Path::new("/home/u/Apps/Cursor.AppImage");
    assert_eq!(
        detect(Path::new("/usr/bin/skyelab"), Some(other), "linux"),
        Some(Install::Deb),
        "deb install must not adopt another app's $APPIMAGE"
    );
    assert_eq!(
        detect(
            Path::new("/home/u/skyelab/target/debug/skyelab"),
            Some(other),
            "linux"
        ),
        None,
        "dev build must not adopt another app's $APPIMAGE"
    );
}
