//! Review fixes: escaping paths are refused by their text before anything touches them, and tracked dotfiles are
//! searched like any other file.

use skyelab_core::files::{inside, search};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[test]
fn inside_refuses_climbing_and_foreign_paths_by_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("sub")).expect("mkdir");
    std::fs::write(dir.path().join("a.rs"), "x").expect("write");
    // Resolves inside, but `..` is never followed.
    assert_eq!(inside(dir.path(), Path::new("sub/../a.rs")), None);
    assert_eq!(
        inside(dir.path(), Path::new("/net/somehost/share/a.rs")),
        None
    );
    assert!(inside(dir.path(), Path::new("a.rs")).is_some());
    let real = dir.path().canonicalize().expect("canonical");
    assert!(inside(dir.path(), &real.join("a.rs")).is_some());
}

#[test]
fn search_finds_text_in_dotfiles_but_not_in_git() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join(".github/workflows")).expect("mkdir");
    std::fs::create_dir(root.join(".git")).expect("mkdir");
    std::fs::write(root.join(".github/workflows/ci.yml"), "run: needle\n").expect("write");
    std::fs::write(root.join(".git/config"), "needle\n").expect("write");
    let found = search(root, "needle", 10, &AtomicBool::new(false)).expect("search");
    let paths: Vec<_> = found.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, [".github/workflows/ci.yml"]);
}
