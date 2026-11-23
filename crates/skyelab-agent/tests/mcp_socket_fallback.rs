//! Acceptance: a run dir too deep for a unix socket path (a long HOME) still gets a working socket, in a short
//! private dir of its own, instead of orchestration silently turning off.
#![cfg(unix)]
use skyelab_agent::mcp::{Caller, Host, Server, Tool};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixStream;
use std::sync::Arc;

struct Nop;
impl Host for Nop {
    fn call(&self, _: &Caller, _: Tool) -> Result<serde_json::Value, String> {
        Ok(serde_json::Value::Null)
    }
}

/// sun_path is 104 bytes on macOS and 108 on Linux, the terminating NUL included.
const SUN_PATH: usize = 104;

#[test]
fn a_long_run_dir_falls_back_to_a_short_private_socket_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let deep = tmp
        .path()
        .join("h".repeat(90))
        .join("skyelab")
        .join("run")
        .join("12345");
    let server = Server::start(&deep, Arc::new(Nop)).expect("start despite the long path");
    let socket = server.socket_path().to_owned();
    assert!(socket.as_os_str().len() < SUN_PATH, "{}", socket.display());
    let dir = socket.parent().expect("socket dir").to_owned();
    assert_ne!(dir, deep);
    let meta = std::fs::symlink_metadata(&dir).expect("socket dir");
    assert!(meta.is_dir() && !meta.file_type().is_symlink());
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    UnixStream::connect(&socket).expect("a client can connect");
    assert!(deep.is_dir(), "grant files still live in the run dir");
    drop(server);
    assert!(!dir.exists(), "the fallback dir outlived the server");
}

#[test]
fn a_short_run_dir_keeps_its_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let run = tmp.path().join("run");
    let server = Server::start(&run, Arc::new(Nop)).expect("start");
    assert_eq!(server.socket_path(), run.join("mcp.sock"));
}
