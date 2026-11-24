//! The folder scans the watcher needs: the folders git can see, and the tracked files an ignore pattern matches.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Tracked files under `toplevel` that an ignore pattern matches, as absolute paths.
pub(super) fn tracked_ignored(toplevel: &Path) -> HashSet<PathBuf> {
    let args = [
        "ls-files",
        "-z",
        "--cached",
        "--ignored",
        "--exclude-standard",
    ];
    let out = crate::git(toplevel, &args).unwrap_or_default();
    out.split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| toplevel.join(String::from_utf8_lossy(p).as_ref()))
        .collect()
}

/// The folders under `root` git can see (gitignored ones and `.git` skipped), up to `cap`, and the `.gitignore`
/// files in them. The flag says the walk stopped at the cap.
pub(super) fn walk(root: &Path, cap: usize) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
    let (mut dirs, mut ignores) = (Vec::new(), Vec::new());
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(true)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for entry in walker.flatten() {
        let Some(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if dirs.len() == cap {
                return (dirs, ignores, true);
            }
            dirs.push(entry.into_path());
        } else if entry.file_name() == ".gitignore" {
            ignores.push(entry.into_path());
        }
    }
    (dirs, ignores, false)
}
