//! A unix socket in a private (0700) dir.

use anyhow::{Context as _, Result};
use std::os::unix::fs::{
    DirBuilderExt as _, MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _,
};
use std::path::{Path, PathBuf};

pub(crate) use std::os::unix::net::{UnixListener as Listener, UnixStream as Conn};

/// sun_path holds 104 bytes on macOS (108 on Linux), the terminating NUL included.
const MAX_SOCKET_PATH: usize = 103;

/// Makes `dir` (already emptied) private to this user: created 0700, then chmod'ed past the umask.
pub(crate) fn private_dir(dir: &Path) -> Result<()> {
    // Private from creation; the chmod after covers a dir that already existed with another mode.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("making {} private", dir.display()))
}

/// A new file only this user can read, failing if it exists.
pub(crate) fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// Listens on `<run_dir>/mcp.sock`, or in a fresh short private dir when that path is too long. Returns the
/// listener, the address the proxy connects to, and the short dir to remove on [`cleanup`].
pub(crate) fn bind(run_dir: &Path, random: &str) -> Result<(Listener, PathBuf, Option<PathBuf>)> {
    let (socket, socket_dir) = match run_dir.join("mcp.sock") {
        s if s.as_os_str().len() <= MAX_SOCKET_PATH => (s, None),
        _ => {
            let short = short_socket_dir(run_dir, random)?;
            (short.join("mcp.sock"), Some(short))
        }
    };
    let listener =
        Listener::bind(&socket).with_context(|| format!("listening on {}", socket.display()))?;
    Ok((listener, socket, socket_dir))
}

pub(crate) fn connect(address: &Path) -> std::io::Result<Conn> {
    Conn::connect(address)
}

/// The server is gone: removes its socket and the short dir, if one was made.
pub(crate) fn cleanup(address: &Path, socket_dir: Option<&Path>) {
    let _ = std::fs::remove_file(address);
    if let Some(d) = socket_dir {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// A fresh dir for the socket when the run dir is too deep (a long HOME): under the temp dir, else `/tmp`.
/// Created exclusively with mode 0700, then checked to be a real dir, 0700 and owned like the run dir (ours),
/// so nobody else can have planted it or can reach the socket.
fn short_socket_dir(run_dir: &Path, random: &str) -> Result<PathBuf> {
    let name = format!("skyelab-{}-{}", std::process::id(), &random[..16]);
    let tmp = std::env::temp_dir().join(&name);
    let dir = if tmp.join("mcp.sock").as_os_str().len() <= MAX_SOCKET_PATH {
        tmp
    } else {
        Path::new("/tmp").join(name)
    };
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    // A strict umask may have taken the owner's bits too.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("making {} private", dir.display()))?;
    let meta = std::fs::symlink_metadata(&dir).context("checking the socket dir")?;
    let owner = std::fs::metadata(run_dir)
        .context("checking the run dir")?
        .uid();
    if !meta.is_dir() || meta.permissions().mode() & 0o777 != 0o700 || meta.uid() != owner {
        anyhow::bail!("{} is not a private dir of ours", dir.display());
    }
    Ok(dir)
}

/// `unsafe` is denied, so no `libc::kill(pid, 0)`: the kill binary asks instead. A reused pid only keeps a dead dir.
pub(crate) fn alive(pid: u32) -> bool {
    std::process::Command::new("/bin/kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map_or(true, |s| s.success()) // can't ask: keep the dir rather than break a live run
}
