//! This install's device key: made once, kept outside every project folder, sent in hello.
//!
//! On macOS it is a keychain item that SkyeLab made, so another program (an agent running `security`, say) gets a
//! system prompt instead of the key. Elsewhere it is a file only this OS user can read, which keeps other users
//! out but not other programs of the same user: the docs say so.

use std::io::{self, Write as _};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

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
            // A copied or restored file may have come back readable by others; it is private from now on.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                if std::fs::metadata(path)?.permissions().mode() & 0o077 != 0 {
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
                }
            }
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

type Loader = dyn Fn() -> Result<String, String> + Send + Sync;

/// This install's device key, read on first use (the first connection to another machine), not at startup: on
/// macOS the read can show a keychain prompt, which a user who never connects anywhere should never see. Clones
/// share one key. A load that fails (the prompt denied) is not kept, so the next connection asks again.
#[derive(Clone)]
pub struct LazyDeviceKey {
    key: Arc<Mutex<Option<String>>>,
    load: Arc<Loader>,
}

impl LazyDeviceKey {
    pub fn new(load: impl Fn() -> Result<String, String> + Send + Sync + 'static) -> Self {
        Self {
            key: Arc::default(),
            load: Arc::new(load),
        }
    }

    /// The key, loading it if no earlier call did. Blocks (the keychain prompt waits for the user), so never call
    /// it on a UI thread. The lock is held while loading, so concurrent callers wait for one load.
    pub fn get(&self) -> Result<String, String> {
        let mut key = self.key.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(key) = &*key {
            return Ok(key.clone());
        }
        let loaded = (self.load)()?;
        *key = Some(loaded.clone());
        Ok(loaded)
    }

    /// The key if it is already loaded; never loads (or waits on a load in progress).
    pub fn peek(&self) -> Option<String> {
        self.key.try_lock().ok().and_then(|k| k.clone())
    }
}

impl From<String> for LazyDeviceKey {
    fn from(key: String) -> Self {
        let this = Self::new(|| Err("unreachable: the key is already loaded".into()));
        *this.key.lock().unwrap_or_else(PoisonError::into_inner) = Some(key);
        this
    }
}

impl From<&str> for LazyDeviceKey {
    fn from(key: &str) -> Self {
        key.to_owned().into()
    }
}

impl std::fmt::Debug for LazyDeviceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the key itself: Options are debug-printed.
        f.write_str("LazyDeviceKey")
    }
}
