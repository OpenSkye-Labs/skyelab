//! Proof: a FIFO the agent puts where the file was can't hang the Preview tab's save or its changed-on-disk check.
//! Opening a FIFO to read blocks until a writer comes, so the save would never finish and every later save would wait.
#![cfg(unix)]

use skyelab_core::files::{changed_on_disk, save_text};
use std::sync::mpsc;
use std::time::Duration;

fn fifo_folder() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let root = std::fs::canonicalize(tmp.path()).expect("canonical");
    let made = std::process::Command::new("mkfifo")
        .arg(root.join("notes.md"))
        .status()
        .expect("mkfifo");
    assert!(made.success(), "mkfifo");
    (tmp, root)
}

/// Runs `f` on a thread; panics when it hasn't returned in 5 s.
fn within_5s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(Duration::from_secs(5))
        .expect("SAVE_HUNG_ON_FIFO")
}

#[test]
fn a_save_over_a_fifo_fails_fast() {
    let (_tmp, root) = fifo_folder();
    let r = within_5s(move || save_text(&root, "notes.md".as_ref(), "x", Some("old")));
    assert!(r.is_err(), "a FIFO is not a file: {r:?}");
}

#[test]
fn an_overwrite_over_a_fifo_fails_fast() {
    let (_tmp, root) = fifo_folder();
    let r = within_5s(move || save_text(&root, "notes.md".as_ref(), "x", None));
    assert!(r.is_err(), "a FIFO is not a file: {r:?}");
}

#[test]
fn the_changed_check_on_a_fifo_answers_fast() {
    let (_tmp, root) = fifo_folder();
    let changed = within_5s(move || changed_on_disk(&root, "notes.md".as_ref(), "old"));
    assert!(changed, "a FIFO where the file was counts as changed");
}

#[test]
fn the_changed_check_reads_a_file() {
    let tmp = tempfile::tempdir().expect("temp dir");
    std::fs::write(tmp.path().join("a.txt"), "same").expect("write");
    assert!(!changed_on_disk(tmp.path(), "a.txt".as_ref(), "same"));
    assert!(
        changed_on_disk(tmp.path(), "a.txt".as_ref(), "sam"),
        "longer on disk"
    );
    assert!(
        changed_on_disk(tmp.path(), "a.txt".as_ref(), "same!"),
        "shorter on disk"
    );
    assert!(
        changed_on_disk(tmp.path(), "gone.txt".as_ref(), "same"),
        "gone counts as changed"
    );
}
