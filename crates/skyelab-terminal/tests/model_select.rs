//! Scrollback and selection on the real model (a PTY child), without a window.
#![cfg(unix)]

use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use futures::channel::mpsc::UnboundedReceiver;
use skyelab_terminal::{Event, Scroll, SelectionType, Side, SpawnOptions, Terminal};

fn pump(term: &mut Terminal, rx: &mut UnboundedReceiver<Event>, done: impl Fn(&Terminal) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
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

fn sh(script: &str) -> (Terminal, UnboundedReceiver<Event>) {
    Terminal::spawn(SpawnOptions {
        cwd: std::env::temp_dir(),
        command: Some(("/bin/sh".into(), vec!["-c".into(), script.into()])),
        env: HashMap::new(),
        cols: 40,
        lines: 10,
    })
    .expect("spawn")
}

/// A script that waits for `flag` to exist, so a test can act between two bursts of output.
fn wait_for(flag: &Path) -> String {
    format!("while [ ! -e '{}' ]; do sleep 0.05; done", flag.display())
}

fn flag(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("skyelab-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn history_is_capped_at_10000() {
    let (mut t, mut rx) = sh("seq 1 12000");
    pump(&mut t, &mut rx, |t| {
        t.exit_status().is_some() && t.text().contains("12000")
    });
    assert_eq!(t.history_size(), 10_000);
}

#[test]
fn scroll_sticks_while_output_arrives() {
    let f = flag("scroll");
    let (mut t, mut rx) = sh(&format!(
        "seq 1 100; {}; seq 101 130; sleep 30",
        wait_for(&f)
    ));
    pump(&mut t, &mut rx, |t| t.text().contains("100"));
    t.scroll(Scroll::Delta(20));
    assert_eq!(t.display_offset(), 20);
    let before = t.text();
    std::fs::write(&f, "").expect("flag");
    pump(&mut t, &mut rx, |t| t.display_offset() == 50);
    assert_eq!(
        t.text(),
        before,
        "the scrolled-up view moved with new output"
    );
    t.scroll(Scroll::Bottom);
    assert_eq!(t.display_offset(), 0);
    let _ = std::fs::remove_file(f);
}

#[test]
fn word_and_line_selection() {
    let (mut t, mut rx) = sh("printf 'alpha beta gamma\\n'; sleep 30");
    pump(&mut t, &mut rx, |t| t.text().contains("gamma"));
    t.select(SelectionType::Semantic, 0, 7, Side::Left);
    assert_eq!(t.selection_text().as_deref(), Some("beta"));
    t.select(SelectionType::Lines, 0, 2, Side::Left);
    assert_eq!(
        t.selection_text().as_deref().map(str::trim_end),
        Some("alpha beta gamma")
    );
    t.select(SelectionType::Simple, 0, 0, Side::Left);
    t.extend_selection(0, 4, Side::Right);
    assert_eq!(t.selection_text().as_deref(), Some("alpha"));
    t.clear_selection();
    assert_eq!(t.selection_text(), None);
}

#[test]
fn selection_survives_output() {
    let f = flag("select");
    let (mut t, mut rx) = sh(&format!("echo first; {}; seq 1 30; sleep 30", wait_for(&f)));
    pump(&mut t, &mut rx, |t| t.text().contains("first"));
    t.select(SelectionType::Simple, 0, 0, Side::Left);
    t.extend_selection(0, 4, Side::Right);
    std::fs::write(&f, "").expect("flag");
    pump(&mut t, &mut rx, |t| {
        t.text().contains("30") && t.history_size() > 0
    });
    assert_eq!(t.selection_text().as_deref(), Some("first"));
    let _ = std::fs::remove_file(f);
}
