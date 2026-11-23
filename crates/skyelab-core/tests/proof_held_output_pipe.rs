//! S0: `scoped_extra_headers` runs on every agent launch. A git whose stdout another process still holds (a
//! sibling that inherited the pipe while it was being created, or anything git left running) must not keep it
//! waiting: the call returns once git exits.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn a_held_stdout_does_not_hang_scoped_extra_headers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let git = tmp.path().join("git");
    // The background sleep keeps git's stdout open for 30 s after git itself is done.
    std::fs::write(
        &git,
        "#!/bin/sh\nsleep 30 &\necho 'http.https://example.com/.extraheader'\n",
    )
    .expect("write fake git");
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let (tx, rx) = mpsc::channel();
    let dir = tmp.path().to_owned();
    std::thread::spawn(move || {
        let _ = tx.send(skyelab_core::env::scoped_extra_headers_with(&git, &dir));
    });
    let urls = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("returned while another process still held git's stdout");
    assert_eq!(urls, vec!["https://example.com/".to_owned()]);
}
