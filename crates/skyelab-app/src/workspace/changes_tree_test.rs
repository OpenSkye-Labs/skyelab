//! Acceptance: the Changes sidebar is a folder tree (collapsible, single-child chains merged), and the split
//! layout pairs removed lines with the added lines after them.

use super::changes_model::{ChangedFile, LineKind, Row};
use super::changes_tree::{SplitRow, TreeRow, split_rows, tree_order, tree_rows};
use skyelab_git::ChangeKind;
use std::collections::HashSet;
use std::path::PathBuf;

fn files(paths: &[&str]) -> Vec<ChangedFile> {
    paths
        .iter()
        .map(|p| ChangedFile {
            path: PathBuf::from(p),
            old_path: None,
            kind: ChangeKind::Modified,
            binary: false,
            first_row: 0,
        })
        .collect()
}

fn labels(rows: &[TreeRow]) -> Vec<String> {
    rows.iter()
        .map(|r| match r {
            TreeRow::Dir {
                label, depth, open, ..
            } => {
                format!(
                    "{}{label}/{}",
                    "  ".repeat(*depth),
                    if *open { "" } else { "+" }
                )
            }
            TreeRow::File { label, depth, .. } => format!("{}{label}", "  ".repeat(*depth)),
        })
        .collect()
}

#[test]
fn tree_rows_nest_and_collapse() {
    let f = files(&["README.md", "src/a.rs", "src/ui/b.rs", "src/ui/c.rs"]);
    let open = tree_rows(&f, &HashSet::new());
    assert_eq!(
        labels(&open),
        [
            "src/",
            "  ui/",
            "    b.rs",
            "    c.rs",
            "  a.rs",
            "README.md"
        ]
    );
    let shut = tree_rows(&f, &HashSet::from([PathBuf::from("src/ui")]));
    assert_eq!(labels(&shut), ["src/", "  ui/+", "  a.rs", "README.md"]);
    let file_ix = shut.iter().find_map(|r| match r {
        TreeRow::File { ix, label, .. } if label == "a.rs" => Some(*ix),
        _ => None,
    });
    assert_eq!(file_ix, Some(1), "a file row points at its changed file");
}

#[test]
fn single_child_dirs_merge() {
    let f = files(&["crates/app/src/main.rs", "crates/app/src/lib.rs"]);
    let rows = tree_rows(&f, &HashSet::new());
    assert_eq!(labels(&rows), ["crates/app/src/", "  lib.rs", "  main.rs"]);
    // The merged row collapses by its full path.
    let rows = tree_rows(&f, &HashSet::from([PathBuf::from("crates/app/src")]));
    assert_eq!(labels(&rows), ["crates/app/src/+"]);
}

#[test]
fn split_rows_pair_removed_and_added() {
    let line = |kind| Row::Line {
        old: None,
        new: None,
        kind,
        text: String::new(),
    };
    let rows = vec![
        Row::File(0),
        Row::Hunk("@@".into()),
        line(LineKind::Context),
        line(LineKind::Removed),
        line(LineKind::Removed),
        line(LineKind::Added),
        line(LineKind::Context),
        line(LineKind::Added),
    ];
    assert_eq!(
        split_rows(&rows),
        [
            SplitRow::Full(0),
            SplitRow::Full(1),
            SplitRow::Pair(Some(2), Some(2)),
            SplitRow::Pair(Some(3), Some(5)),
            SplitRow::Pair(Some(4), None),
            SplitRow::Pair(Some(6), Some(6)),
            SplitRow::Pair(None, Some(7)),
        ]
    );
}

#[test]
fn the_diff_lists_files_in_the_trees_order() {
    let mut f = files(&[
        "README.md",
        "agent-edit.txt",
        "docs/guide.md",
        "src/deep/nested/b.txt",
        "src/f1.rs",
    ]);
    f.sort_by(|a, b| tree_order(&a.path, &b.path));
    let shown: Vec<usize> = tree_rows(&f, &HashSet::new())
        .iter()
        .filter_map(|r| match r {
            TreeRow::File { ix, .. } => Some(*ix),
            TreeRow::Dir { .. } => None,
        })
        .collect();
    assert_eq!(shown, [0, 1, 2, 3, 4], "top to bottom, the tree and the diff agree");
    let order: Vec<_> = f.iter().map(|f| f.path.to_string_lossy().into_owned()).collect();
    assert_eq!(
        order,
        ["docs/guide.md", "src/deep/nested/b.txt", "src/f1.rs", "README.md", "agent-edit.txt"]
    );
}
