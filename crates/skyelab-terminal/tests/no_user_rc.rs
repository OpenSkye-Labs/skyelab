//! A test terminal never reads the developer's shell setup: the default shell (no command) is started with a HOME
//! whose every rc file writes a marker, and none appears. Without the `test-support` shell this runs `$SHELL -l`,
//! which reads them.
#![cfg(unix)]

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use skyelab_terminal::{SpawnOptions, Terminal};

#[test]
fn the_default_shell_reads_no_rc_file() {
    let home = tempfile::tempdir().expect("temp home");
    let marker = home.path().join("rc-was-read");
    let line = format!("echo read >> '{}'\n", marker.display());
    for rc in [
        ".zshenv",
        ".zprofile",
        ".zshrc",
        ".zlogin",
        ".profile",
        ".bash_profile",
        ".bash_login",
        ".bashrc",
        ".kshrc",
        ".config/fish/config.fish",
    ] {
        let path = home.path().join(rc);
        std::fs::create_dir_all(path.parent().expect("in home")).expect("rc dir");
        std::fs::write(path, &line).expect("rc file");
    }
    let dir = |d: &std::path::Path| d.to_str().expect("utf-8 temp path").to_owned();
    let (term, mut rx) = Terminal::spawn(SpawnOptions {
        cwd: home.path().to_owned(),
        command: None,
        env: HashMap::from([
            ("HOME".into(), dir(home.path())),
            ("ZDOTDIR".into(), dir(home.path())),
            ("XDG_CONFIG_HOME".into(), dir(&home.path().join(".config"))),
        ]),
        cols: 80,
        lines: 24,
    })
    .expect("spawn");
    // The shell ran this line, so any rc it was going to read has been read by now.
    let done = home.path().join("command-ran");
    term.write(format!("touch '{}'; exit\n", done.display()).into_bytes());
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done.exists() {
        assert!(Instant::now() < deadline, "the shell never ran the command");
        while rx.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!marker.exists(), "the test shell read an rc file from HOME");
}
