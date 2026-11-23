//! S3: on Windows every agent tab gets its own WebView2 user-data folder, so InPrivate sessions (cookies,
//! storage) are never shared between agent tabs or accounts. The folder chooser runs on every OS.

#[test]
fn two_agent_tabs_get_two_private_dirs_removed_with_their_tab() {
    let a = skyelab_browser::agent_data_dir().expect("first dir");
    let b = skyelab_browser::agent_data_dir().expect("second dir");
    assert_ne!(a.path(), b.path());
    assert!(a.path().is_dir() && b.path().is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(a.path())
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "only the user can read it");
    }
    let gone = a.path().to_owned();
    drop(a);
    assert!(!gone.exists(), "removed with its tab");
}

#[test]
fn only_windows_agent_tabs_get_a_folder_and_an_override_refuses_them() {
    use skyelab_browser::webview_data_dir;
    use std::ffi::OsStr;
    assert!(
        webview_data_dir(false, true, None)
            .expect("mac/linux")
            .is_none()
    );
    assert!(
        webview_data_dir(true, false, None)
            .expect("user tab")
            .is_none()
    );
    assert!(
        webview_data_dir(true, true, None)
            .expect("agent tab")
            .is_some()
    );
    assert!(
        webview_data_dir(true, true, Some(OsStr::new("")))
            .expect("empty")
            .is_some()
    );
    let err = webview_data_dir(true, true, Some(OsStr::new("C:\\shared"))).expect_err("override");
    assert!(
        err.to_string().contains("WEBVIEW2_USER_DATA_FOLDER"),
        "{err}"
    );
}
