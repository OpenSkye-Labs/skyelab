//! Where SkyeLab keeps its files, and one running copy per data dir.
//!
//! Debug builds use their own dirs (`skyelab-dev`), so a dev branch's store migration never reaches the installed
//! app's `skyelab.db`. Within one kind of build, the first launch takes an OS file lock on `skyelab.lock` in the data
//! dir; the OS drops it when the process ends, crash included, so a lock is never stale. That copy listens on
//! 127.0.0.1 and writes its port and a random token to `skyelab.instance` (0600); a second launch finds the lock
//! held, asks the first to show a window, and exits.

use anyhow::{Context as _, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

/// The folder name under the platform's config, data and cache dirs.
pub const APP_DIR: &str = if cfg!(debug_assertions) {
    "skyelab-dev"
} else {
    "skyelab"
};

/// `name` in this build's config dir (accounts, settings, keybindings).
pub fn config_file(name: &str) -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|d| d.join(APP_DIR).join(name))
}

const LOCK: &str = "skyelab.lock";
const INFO: &str = "skyelab.instance";
/// What a second launch sends after the token.
const SHOW: &str = "show";

/// The outcome of [`claim`].
pub enum Claim {
    /// This process is the one for the dir.
    First(Instance),
    /// Another process holds the dir; it was asked to show a window.
    Running,
    /// Another process holds the dir but didn't take the request (still starting, or hung). Never start a second
    /// copy on its store.
    Unreachable(anyhow::Error),
}

/// The lock and the listener a second launch reaches; both go when this drops (or the process ends).
pub struct Instance {
    _lock: File,
    listener: TcpListener,
    token: String,
}

/// Takes `dir` for this process, or, when another process holds it, asks that one to show a window.
pub fn claim(dir: &Path) -> Result<Claim> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    // Private: another user who could open the lock could hold it and keep SkyeLab from starting.
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .with_context(|| format!("making {} private", dir.display()))?;
    let mut opts = OpenOptions::new();
    opts.create(true).truncate(false).read(true).write(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let lock = opts
        .open(dir.join(LOCK))
        .with_context(|| format!("opening {}", dir.join(LOCK).display()))?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            return Ok(match ask_to_show(dir) {
                Ok(()) => Claim::Running,
                Err(e) => Claim::Unreachable(e),
            });
        }
        Err(std::fs::TryLockError::Error(e)) => {
            return Err(e).with_context(|| format!("locking {}", dir.join(LOCK).display()));
        }
    }
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).context("listening for a second launch")?;
    let token = token()?;
    let port = listener.local_addr()?.port();
    write_private(&dir.join(INFO), &format!("{port} {token}\n"))?;
    Ok(Claim::First(Instance {
        _lock: lock,
        listener,
        token,
    }))
}

impl Instance {
    /// Blocks until a second launch asks to be shown (`Ok(())`); other connections are ignored.
    pub fn wait_for_show(&self) -> Result<()> {
        let want = format!("{} {SHOW}", self.token);
        loop {
            let (conn, _) = self.listener.accept().context("accepting")?;
            if read_request(conn) == want {
                return Ok(());
            }
        }
    }
}

/// The most a request may take, so a slow or silent peer can't hold the next launch back for long.
const REQUEST_DEADLINE: Duration = Duration::from_secs(1);

/// What a peer sent within [`REQUEST_DEADLINE`], at most 256 bytes (more is not a second launch).
fn read_request(mut conn: TcpStream) -> String {
    let deadline = std::time::Instant::now() + REQUEST_DEADLINE;
    let mut got = Vec::new();
    let mut buf = [0u8; 64];
    while got.len() < 256 {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() || conn.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match conn.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&got).trim().to_owned()
}

/// The running copy's port and token, retried for about 2 s: it writes them right after taking the lock, so a
/// launch in between reads none, or a crashed run's stale port.
fn ask_to_show(dir: &Path) -> Result<()> {
    let mut last = anyhow::anyhow!("no address");
    for _ in 0..20 {
        match send_show(dir) {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(last.context(format!(
        "SkyeLab holds {} but can't be reached",
        dir.display()
    )))
}

fn send_show(dir: &Path) -> Result<()> {
    let text = std::fs::read_to_string(dir.join(INFO))?;
    let (port, token) = text.trim().split_once(' ').context("a malformed address")?;
    let port: u16 = port.parse()?;
    let mut conn = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    conn.write_all(format!("{token} {SHOW}").as_bytes())?;
    Ok(())
}

fn token() -> Result<String> {
    use ring::rand::SecureRandom as _;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| anyhow::anyhow!("no system randomness"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Writes `text` to `path`, 0600 on unix (the token lets a process ask for a window; on Windows the data dir's
/// inherited ACL keeps it private).
fn write_private(path: &Path, text: &str) -> Result<()> {
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts
        .open(path)
        .with_context(|| format!("writing {}", path.display()))?;
    f.write_all(text.as_bytes())?;
    Ok(())
}
