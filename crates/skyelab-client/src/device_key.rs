//! This install's device key: made once, kept outside every project folder, sent in hello.
//!
//! On macOS it is a keychain item that SkyeLab made, so another program (an agent running `security`, say) gets a
//! system prompt instead of the key. Elsewhere it is a file only this OS user can read, which keeps other users
//! out but not other programs of the same user: the docs say so.

use std::io::{self, Write as _};
use std::path::Path;

use skyelab_protocol::{device_code, device_key_hash, is_device_key, new_device_key};

pub(crate) fn code(key: &str) -> String {
    device_code(&device_key_hash(key))
}

fn not_a_key(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{what} doesn't hold a SkyeLab device key; remove it to make a new one (then approve this device again)"
        ),
    )
}

/// The key in `path`, made (0600, and its folder) if there is none yet. A file that holds something else is an
/// error, never replaced: a new key would need approving again everywhere.
pub fn load_or_create_device_key(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let key = text.trim();
            return if is_device_key(key) {
                Ok(key.to_owned())
            } else {
                Err(not_a_key(&path.display().to_string()))
            };
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let key = new_device_key()?;
    let mut file = private_new_file(path)?;
    file.write_all(format!("{key}\n").as_bytes())?;
    file.sync_all()?;
    Ok(key)
}

/// Fails if the file already exists, so two first launches can't each write their own key.
fn private_new_file(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

#[cfg(target_os = "macos")]
const SERVICE: &str = "dev.openskye.skyelab";
#[cfg(target_os = "macos")]
const ACCOUNT: &str = "device-key";

/// The key in the user's default keychain, made there if there is none yet.
#[cfg(target_os = "macos")]
pub fn keychain_device_key() -> io::Result<String> {
    let keychain = security_framework::os::macos::keychain::SecKeychain::default()
        .map_err(io::Error::other)?;
    keychain_device_key_in(&keychain)
}

/// The key in `keychain`, made there if there is none yet. The item's access list is the default one: the
/// program that made it (SkyeLab) reads it freely, any other program needs the user's yes.
#[cfg(target_os = "macos")]
pub fn keychain_device_key_in(
    keychain: &security_framework::os::macos::keychain::SecKeychain,
) -> io::Result<String> {
    /// errSecItemNotFound
    const NOT_FOUND: i32 = -25300;
    match keychain.find_generic_password(SERVICE, ACCOUNT) {
        Ok((password, _item)) => {
            let key = std::str::from_utf8(password.as_ref()).unwrap_or_default();
            if is_device_key(key) {
                Ok(key.to_owned())
            } else {
                Err(not_a_key("the keychain item dev.openskye.skyelab"))
            }
        }
        Err(e) if e.code() == NOT_FOUND => {
            let key = new_device_key()?;
            keychain
                .add_generic_password(SERVICE, ACCOUNT, key.as_bytes())
                .map_err(io::Error::other)?;
            Ok(key)
        }
        Err(e) => Err(io::Error::other(e)),
    }
}
