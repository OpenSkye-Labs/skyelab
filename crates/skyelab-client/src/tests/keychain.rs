//! On macOS the desktop's device key lives in the keychain, as an item only SkyeLab itself reads without a
//! prompt. Tested against a throwaway keychain file, never the user's login keychain.
#![cfg(target_os = "macos")]

use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};

/// A new, unlocked keychain in a temp dir, removed from the system's lists when dropped.
struct TempKeychain {
    dir: tempfile::TempDir,
    path: std::path::PathBuf,
}

impl TempKeychain {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test.keychain");
        let mut keychain = CreateOptions::new()
            .password("test")
            .prompt_user(false)
            .create(&path)
            .expect("create keychain");
        keychain.unlock(Some("test")).expect("unlock");
        Self { dir, path }
    }
}

impl Drop for TempKeychain {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/usr/bin/security")
            .arg("delete-keychain")
            .arg(&self.path)
            .status();
    }
}

#[test]
fn the_key_is_made_once_in_the_keychain_and_no_file_is_written() {
    let temp = TempKeychain::new();
    // Fails instead of showing a dialog if anything here would need the user.
    let _no_prompts = SecKeychain::disable_user_interaction().expect("no prompts");
    let keychain = SecKeychain::open(&temp.path).expect("open");
    let key = crate::keychain_device_key_in(&keychain).expect("create");
    assert!(skyelab_protocol::is_device_key(&key), "{key}");
    assert_eq!(crate::keychain_device_key_in(&keychain).expect("load"), key);
    // Only the keychain's own files are here, and none of them holds the key in the clear.
    for entry in std::fs::read_dir(temp.dir.path()).expect("list") {
        let path = entry.expect("entry").path();
        let bytes = std::fs::read(&path).unwrap_or_default();
        assert!(
            !bytes.windows(key.len()).any(|w| w == key.as_bytes()),
            "{} holds the key in plain text",
            path.display()
        );
    }
}
