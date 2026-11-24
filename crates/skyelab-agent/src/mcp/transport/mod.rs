//! The one platform-specific part of the MCP server: how the app listens and the proxy connects. Both sides expose
//! the same names, and `Conn` has `UnixStream`'s methods (`try_clone`, `set_read_timeout`, `shutdown`), so the
//! server and proxy code is shared.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::create_private_file;
#[cfg(unix)]
pub(super) use unix::*;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::create_private_file;
#[cfg(windows)]
pub(super) use windows::*;
