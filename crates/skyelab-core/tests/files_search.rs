//! Content search: grouped, capped, cancellable, and blind to ignored and binary files.

use skyelab_core::files::{LineMatch, search, walk};
use std::sync::atomic::AtomicBool;

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir(root.join(".git")).expect("mkdir"); // .gitignore only counts in a repo
    std::fs::write(root.join(".gitignore"), "target/\n").expect("write");
    std::fs::create_dir(root.join("src")).expect("mkdir");
    std::fs::create_dir(root.join("target")).expect("mkdir");
    std::fs::write(
        root.join("src/a.rs"),
        "let needle = 1;\nnothing\nNEEDLE again\n",
    )
    .expect("write");
    std::fs::write(root.join("src/b.rs"), "needle\n").expect("write");
    std::fs::write(root.join("target/out.rs"), "needle\n").expect("write");
    std::fs::write(root.join("blob.bin"), b"needle\0\x01").expect("write");
    dir
}

#[test]
fn search_groups_matches_and_caps() {
    let dir = fixture();
    let found = search(dir.path(), "needle", 100, &AtomicBool::new(false)).expect("search");
    let paths: Vec<_> = found.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["src/a.rs", "src/b.rs"]);
    assert_eq!(
        found.files[0].lines,
        [
            LineMatch {
                line: 1,
                text: "let needle = 1;".into()
            },
            LineMatch {
                line: 3,
                text: "NEEDLE again".into()
            },
        ]
    );
    assert!(!found.truncated);

    // An uppercase letter makes it case-sensitive.
    let found = search(dir.path(), "NEEDLE", 100, &AtomicBool::new(false)).expect("search");
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].lines.len(), 1);

    let found = search(dir.path(), "needle", 2, &AtomicBool::new(false)).expect("search");
    assert_eq!(found.files.len(), 1);
    assert_eq!(found.files[0].lines.len(), 2);
    assert!(found.truncated);

    // Literal, not a regex.
    let found = search(dir.path(), "needle.*", 100, &AtomicBool::new(false)).expect("search");
    assert!(found.files.is_empty());
}

#[test]
fn search_stops_when_cancelled() {
    let dir = fixture();
    let found = search(dir.path(), "needle", 100, &AtomicBool::new(true)).expect("search");
    assert!(found.files.is_empty());
}

#[test]
fn walk_skips_ignored_and_hidden() {
    let dir = fixture();
    assert_eq!(walk(dir.path(), 100), ["blob.bin", "src/a.rs", "src/b.rs"]);
    assert_eq!(walk(dir.path(), 1), ["blob.bin"]);
}

#[cfg(unix)]
#[test]
fn search_does_not_follow_symlinks_out() {
    let outer = tempfile::tempdir().expect("tempdir");
    let root = outer.path().join("repo");
    std::fs::create_dir(&root).expect("mkdir");
    std::fs::create_dir(outer.path().join("elsewhere")).expect("mkdir");
    std::fs::write(outer.path().join("elsewhere/secret.txt"), "needle\n").expect("write");
    std::os::unix::fs::symlink(outer.path().join("elsewhere"), root.join("link")).expect("symlink");
    std::os::unix::fs::symlink(
        outer.path().join("elsewhere/secret.txt"),
        root.join("file-link.txt"),
    )
    .expect("symlink");
    let found = search(&root, "needle", 100, &AtomicBool::new(false)).expect("search");
    assert!(found.files.is_empty(), "{found:?}");
    assert!(walk(&root, 100).is_empty());
}
