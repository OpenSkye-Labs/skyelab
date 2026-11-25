//! Proof: the Preview tab's save never writes outside the thread's folder through its temp file or a swapped parent
//! folder. The agent controls the folder: it can plant a symlink where the temp file will go (it knows SkyeLab's pid
//! as $PPID) and swap a parent folder for a symlink while the save runs.
#![cfg(unix)]

use skyelab_core::files::save_text;
use std::fs;
use std::os::unix::fs::symlink;

fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let parent = tempfile::tempdir().expect("temp dir");
    let base = fs::canonicalize(parent.path()).expect("canonical");
    let root = base.join("proj");
    fs::create_dir(&root).expect("root");
    let outside = base.join("outside.txt");
    fs::write(&outside, "keep").expect("outside");
    (parent, root, outside)
}

#[test]
fn a_symlink_at_the_temp_name_is_not_followed() {
    let (_tmp, root, outside) = setup();
    fs::write(root.join("notes.md"), "old").expect("notes");
    // The name the first version used: `.<name>.<pid>.tmp`.
    symlink(
        &outside,
        root.join(format!(".notes.md.{}.tmp", std::process::id())),
    )
    .expect("plant");
    // Its name before that: `<stem>.toml.<pid>.tmp`.
    symlink(
        &outside,
        root.join(format!("notes.toml.{}.tmp", std::process::id())),
    )
    .expect("plant");
    let r = save_text(&root, "notes.md".as_ref(), "PWNED", Some("old"));
    assert_eq!(
        fs::read_to_string(&outside).expect("read"),
        "keep",
        "nothing written outside"
    );
    assert!(r.is_ok(), "the save still works: {r:?}");
    assert_eq!(
        fs::read_to_string(root.join("notes.md")).expect("read"),
        "PWNED"
    );
}

#[test]
fn symlinks_at_guessed_temp_names_are_not_followed() {
    let (_tmp, root, outside) = setup();
    fs::write(root.join("notes.md"), "old").expect("notes");
    // Guesses at any short random or counter suffix the temp name might use.
    for i in 0..2000u32 {
        for name in [
            format!(".notes.md.{i}.tmp"),
            format!(".notes.md.{i:x}.tmp"),
            format!(".notes.md{i}.tmp"),
            format!(".tmp{i}"),
        ] {
            let _ = symlink(&outside, root.join(name));
        }
    }
    for _ in 0..20 {
        let _ = save_text(&root, "notes.md".as_ref(), "PWNED", None);
    }
    assert_eq!(
        fs::read_to_string(&outside).expect("read"),
        "keep",
        "nothing written outside"
    );
}

#[test]
fn a_parent_swapped_for_a_symlink_mid_save_is_not_followed() {
    let (_tmp, root, _outside) = setup();
    let base = root.parent().expect("base").to_path_buf();
    let elsewhere = base.join("elsewhere");
    fs::create_dir(&elsewhere).expect("elsewhere");
    fs::write(elsewhere.join("notes.md"), "keep").expect("victim");
    let real = base.join("real-sub");
    fs::create_dir(&real).expect("real");
    fs::write(real.join("notes.md"), "old").expect("notes");
    let sub = root.join("sub");
    fs::rename(&real, &sub).expect("sub");
    // The agent swaps `sub` for a link out and back, as fast as it can.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let swapper = {
        let (stop, sub, elsewhere, real) =
            (stop.clone(), sub.clone(), elsewhere.clone(), real.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                if fs::rename(&sub, &real).is_ok() {
                    let _ = symlink(&elsewhere, &sub);
                    std::thread::yield_now();
                    let _ = fs::remove_file(&sub);
                    let _ = fs::rename(&real, &sub);
                }
            }
        })
    };
    for _ in 0..3000 {
        let _ = save_text(&root, "sub/notes.md".as_ref(), "PWNED", None);
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    swapper.join().expect("swapper");
    let victim = fs::read_to_string(elsewhere.join("notes.md")).expect("read");
    assert_eq!(victim, "keep", "nothing written through the swapped folder");
    let leftovers: Vec<_> = fs::read_dir(&elsewhere)
        .expect("list")
        .filter_map(Result::ok)
        .map(|e| e.file_name())
        .collect();
    assert_eq!(
        leftovers.len(),
        1,
        "no temp file made outside: {leftovers:?}"
    );
}
