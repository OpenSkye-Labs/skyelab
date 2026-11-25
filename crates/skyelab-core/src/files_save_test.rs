//! Acceptance: the Preview tab's saves write only inside the thread's folder, atomically, keeping the file's
//! permissions, and never over a file that changed on disk since it was loaded.

use super::files::{Saved, save_text};
use std::fs;

#[test]
fn save_writes_when_the_file_is_as_loaded() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "old\r\n").unwrap();
    let r = save_text(dir.path(), "a.txt".as_ref(), "new\r\n", Some("old\r\n")).unwrap();
    assert_eq!(r, Saved::Written);
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "new\r\n"
    );
    let leftovers: Vec<_> = fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(leftovers.len(), 1, "no temp file left behind");
}

#[test]
fn save_reports_a_changed_file() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "agent wrote this").unwrap();
    let r = save_text(dir.path(), "a.txt".as_ref(), "mine", Some("old")).unwrap();
    assert_eq!(r, Saved::Changed, "the disk differs from what was loaded");
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "agent wrote this",
        "nothing written"
    );
    // Overwrite: no base to compare.
    assert_eq!(
        save_text(dir.path(), "a.txt".as_ref(), "mine", None).unwrap(),
        Saved::Written
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn save_refuses_dotdot_and_foreign_paths() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("proj");
    fs::create_dir(&root).unwrap();
    fs::write(parent.path().join("outside.txt"), "keep").unwrap();
    assert!(save_text(&root, "../outside.txt".as_ref(), "x", None).is_err());
    assert!(save_text(&root, &parent.path().join("outside.txt"), "x", None).is_err());
    assert_eq!(
        fs::read_to_string(parent.path().join("outside.txt")).unwrap(),
        "keep"
    );
}

#[cfg(unix)]
#[test]
fn save_refuses_a_symlink_out() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("proj");
    fs::create_dir(&root).unwrap();
    let outside = parent.path().join("secret.txt");
    fs::write(&outside, "keep").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link.txt")).unwrap();
    std::os::unix::fs::symlink(parent.path(), root.join("up")).unwrap();
    assert!(save_text(&root, "link.txt".as_ref(), "x", None).is_err());
    assert!(save_text(&root, "up/secret.txt".as_ref(), "x", None).is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep");
    assert!(
        fs::symlink_metadata(root.join("link.txt"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link itself is untouched"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_folder_saves_through_to_its_target() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("real.txt"), "old").unwrap();
    std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("alias.txt")).unwrap();
    save_text(dir.path(), "alias.txt".as_ref(), "new", Some("old")).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("real.txt")).unwrap(),
        "new"
    );
    assert!(
        fs::symlink_metadata(dir.path().join("alias.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn save_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("run.sh");
    fs::write(&file, "#!/bin/sh\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o751)).unwrap();
    save_text(dir.path(), "run.sh".as_ref(), "#!/bin/sh\necho hi\n", None).unwrap();
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o751
    );
}

#[test]
fn save_refuses_a_missing_file_and_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    assert!(save_text(dir.path(), "gone.txt".as_ref(), "x", None).is_err());
    assert!(save_text(dir.path(), "sub".as_ref(), "x", None).is_err());
    assert!(!dir.path().join("gone.txt").exists());
}
