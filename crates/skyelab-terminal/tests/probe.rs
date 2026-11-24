//! The probe behind tab titles, the close check and restore: the command in front of the shell and its folder.
#![cfg(unix)]

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use skyelab_terminal::{SpawnOptions, Terminal};

fn wait(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_running_command_is_busy_and_named_and_the_folder_follows_cd() {
    let start = tempfile::tempdir().expect("temp dir");
    let (term, _rx) = Terminal::spawn(SpawnOptions {
        cwd: start.path().to_path_buf(),
        // An interactive shell puts each command in its own foreground group, as a login shell does.
        command: Some(("/bin/sh".into(), vec!["-i".into()])),
        env: HashMap::new(),
        cols: 80,
        lines: 24,
    })
    .expect("spawn");
    let probe = term.probe().expect("a live shell has a probe");
    wait("the prompt", || !probe.is_busy());
    assert_eq!(probe.foreground().command, None, "at the prompt");

    term.write(b"cd /\n".as_slice());
    wait("the cd", || {
        probe.foreground().cwd.as_deref() == Some(std::path::Path::new("/"))
    });

    term.write(b"sleep 30\n".as_slice());
    // Busy shows as soon as the shell forks; the name only once the child has exec'd sleep.
    wait("sleep in front", || {
        probe.is_busy() && probe.foreground().command.as_deref() == Some("sleep")
    });

    term.write(b"\x03".as_slice()); // ctrl-c
    wait("the prompt again", || !probe.is_busy());
    term.shutdown();
}
