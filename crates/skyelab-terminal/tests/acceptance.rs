use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use alacritty_terminal::term::TermMode;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::Keystroke;
use skyelab_terminal::{Event, SpawnOptions, Terminal, key_to_bytes};

/// Feeds events into the model until `done` holds or 10s pass.
fn pump(term: &mut Terminal, rx: &mut UnboundedReceiver<Event>, done: impl Fn(&Terminal) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(term) {
        assert!(
            Instant::now() < deadline,
            "timed out; grid:\n{}",
            term.text()
        );
        match rx.try_recv() {
            Ok(ev) => term.handle_event(&ev),
            Err(_) => std::thread::sleep(Duration::from_millis(10)),
        }
    }
}

fn spawn(
    program: &str,
    args: &[&str],
    env: HashMap<String, String>,
) -> (Terminal, UnboundedReceiver<Event>) {
    Terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        command: Some((program.into(), args.iter().map(|a| a.to_string()).collect())),
        env,
        cols: 80,
        lines: 24,
    })
    .expect("spawn")
}

#[cfg(unix)]
#[test]
fn echo_hello_reaches_grid() {
    let (mut term, mut rx) = spawn("echo", &["hello"], HashMap::new());
    pump(&mut term, &mut rx, |t| {
        t.exit_status().is_some() && t.text().contains("hello")
    });
    assert!(term.exit_status().is_some_and(|s| s.success()));
}

#[cfg(unix)]
#[test]
fn resize_changes_grid() {
    let (mut term, _rx) = spawn("sleep", &["5"], HashMap::new());
    assert_eq!(term.size(), (80, 24));
    term.resize(40, 10, 8.0, 16.0);
    assert_eq!(term.size(), (40, 10));
}

#[cfg(unix)]
#[test]
fn env_reaches_child() {
    let env = HashMap::from([("SKYELAB_TEST_VAR".to_string(), "bound-account".to_string())]);
    let (mut term, mut rx) = spawn("printenv", &["SKYELAB_TEST_VAR"], env);
    pump(&mut term, &mut rx, |t| t.text().contains("bound-account"));
}

#[test]
fn key_table() {
    let k = |s: &'static str, mode: TermMode| {
        let Ok(keystroke) = Keystroke::parse(s) else {
            panic!("bad keystroke {s}")
        };
        key_to_bytes(&keystroke, mode)
    };
    let n = TermMode::NONE;
    assert_eq!(k("enter", n).as_deref(), Some(&b"\r"[..]));
    assert_eq!(k("backspace", n).as_deref(), Some(&b"\x7f"[..]));
    assert_eq!(k("tab", n).as_deref(), Some(&b"\t"[..]));
    assert_eq!(k("escape", n).as_deref(), Some(&b"\x1b"[..]));
    assert_eq!(k("ctrl-c", n).as_deref(), Some(&b"\x03"[..]));
    assert_eq!(k("up", n).as_deref(), Some(&b"\x1b[A"[..]));
    assert_eq!(
        k("up", TermMode::APP_CURSOR).as_deref(),
        Some(&b"\x1bOA"[..])
    );
    assert_eq!(k("left", n).as_deref(), Some(&b"\x1b[D"[..]));
    assert_eq!(k("alt-b", n).as_deref(), Some(&b"\x1bb"[..]));
    assert_eq!(k("a", n).as_deref(), Some(&b"a"[..]));
    assert_eq!(k("cmd-v", n), None);
}
