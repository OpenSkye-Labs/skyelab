//! The folder guard: nothing outside a thread's folder resolves, however the path gets there.

use skyelab_core::files::{inside, read_preview};
use std::path::Path;

#[test]
fn inside_allows_files_in_the_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("src")).expect("mkdir");
    std::fs::write(dir.path().join("src/a.rs"), "fn a() {}\n").expect("write");
    let real = dir.path().canonicalize().expect("canonical");
    assert_eq!(
        inside(dir.path(), Path::new("src/a.rs")),
        Some(real.join("src/a.rs"))
    );
    assert_eq!(
        inside(dir.path(), &dir.path().join("src/a.rs")),
        Some(real.join("src/a.rs"))
    );
    assert_eq!(inside(dir.path(), Path::new("src/missing.rs")), None);
}

#[test]
fn inside_refuses_dotdot_and_absolute_escapes() {
    let outer = tempfile::tempdir().expect("tempdir");
    let root = outer.path().join("repo");
    std::fs::create_dir(&root).expect("mkdir");
    std::fs::write(outer.path().join("secret.txt"), "no").expect("write");
    assert_eq!(inside(&root, Path::new("../secret.txt")), None);
    assert_eq!(inside(&root, &outer.path().join("secret.txt")), None);
    assert!(read_preview(&root, Path::new("../secret.txt")).is_err());
}

#[cfg(unix)]
#[test]
fn inside_refuses_symlink_escape() {
    let outer = tempfile::tempdir().expect("tempdir");
    let root = outer.path().join("repo");
    std::fs::create_dir(&root).expect("mkdir");
    std::fs::write(outer.path().join("secret.txt"), "no").expect("write");
    std::os::unix::fs::symlink(outer.path().join("secret.txt"), root.join("link.txt"))
        .expect("symlink");
    std::os::unix::fs::symlink(outer.path(), root.join("up")).expect("symlink");
    assert_eq!(inside(&root, Path::new("link.txt")), None);
    assert_eq!(inside(&root, Path::new("up/secret.txt")), None);
    assert!(read_preview(&root, Path::new("link.txt")).is_err());
    // A link that stays inside is fine.
    std::fs::write(root.join("real.txt"), "yes").expect("write");
    std::os::unix::fs::symlink(root.join("real.txt"), root.join("alias.txt")).expect("symlink");
    assert!(inside(&root, Path::new("alias.txt")).is_some());
}
