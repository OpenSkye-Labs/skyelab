//! The Changes tab's two derived layouts: the file sidebar as a folder tree (T3's DiffFileTree) and the diff as
//! side-by-side rows (T3's split layout). Both are plain functions of the loaded rows, so they are tested without a
//! window and rebuilt only when the diff or a folder's state changes.

use super::changes_model::{ChangedFile, LineKind, Row};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// One row of the file tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRow {
    /// A folder: its repo-relative path (the key for collapsing) and the name shown, which joins a chain of
    /// folders that hold only one folder each (`src/app`).
    Dir {
        path: PathBuf,
        label: String,
        depth: usize,
        open: bool,
    },
    /// Index into the changed files.
    File {
        ix: usize,
        label: String,
        depth: usize,
    },
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<(String, usize)>,
}

/// The tree's visible rows: folders first, then files, each level sorted by name; a collapsed folder hides what is
/// under it.
pub fn tree_rows(files: &[ChangedFile], collapsed: &HashSet<PathBuf>) -> Vec<TreeRow> {
    let mut root = Node::default();
    for (ix, f) in files.iter().enumerate() {
        let mut parts: Vec<String> = f
            .path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some(name) = parts.pop() else { continue };
        let node = parts
            .into_iter()
            .fold(&mut root, |n, p| n.dirs.entry(p).or_default());
        node.files.push((name, ix));
    }
    let mut out = Vec::new();
    walk(&root, Path::new(""), 0, collapsed, &mut out);
    out
}

fn walk(
    node: &Node,
    at: &Path,
    depth: usize,
    collapsed: &HashSet<PathBuf>,
    out: &mut Vec<TreeRow>,
) {
    for (name, mut dir) in &node.dirs {
        let (mut path, mut label) = (at.join(name), name.clone());
        while dir.files.is_empty() && dir.dirs.len() == 1 {
            let (next, child) = dir.dirs.iter().next().expect("checked: exactly one child");
            path.push(next);
            label = format!("{label}/{next}");
            dir = child;
        }
        let open = !collapsed.contains(&path);
        out.push(TreeRow::Dir {
            path: path.clone(),
            label,
            depth,
            open,
        });
        if open {
            walk(dir, &path, depth + 1, collapsed, out);
        }
    }
    let mut files = node.files.clone();
    files.sort();
    out.extend(
        files
            .into_iter()
            .map(|(label, ix)| TreeRow::File { ix, label, depth }),
    );
}

/// One row of the side-by-side diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitRow {
    /// A full-width row (a file header, a hunk header, a placeholder): index into the rows.
    Full(usize),
    /// Old on the left, new on the right; a context line is both sides. Indexes into the rows.
    Pair(Option<usize>, Option<usize>),
}

impl SplitRow {
    /// Whether this row shows `row`.
    pub fn shows(&self, row: usize) -> bool {
        match *self {
            SplitRow::Full(r) => r == row,
            SplitRow::Pair(l, r) => l == Some(row) || r == Some(row),
        }
    }
}

/// The rows side by side: each run of removed lines is paired, in order, with the added lines that follow it.
pub fn split_rows(rows: &[Row]) -> Vec<SplitRow> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let kind = |ix: usize| match rows.get(ix) {
            Some(Row::Line { kind, .. }) => Some(*kind),
            _ => None,
        };
        match kind(i) {
            Some(LineKind::Context) => {
                out.push(SplitRow::Pair(Some(i), Some(i)));
                i += 1;
            }
            Some(LineKind::Removed | LineKind::Added) => {
                let removed: Vec<usize> = (i..)
                    .take_while(|&j| kind(j) == Some(LineKind::Removed))
                    .collect();
                let after = i + removed.len();
                let added: Vec<usize> = (after..)
                    .take_while(|&j| kind(j) == Some(LineKind::Added))
                    .collect();
                for k in 0..removed.len().max(added.len()) {
                    out.push(SplitRow::Pair(
                        removed.get(k).copied(),
                        added.get(k).copied(),
                    ));
                }
                i = after + added.len();
            }
            None => {
                out.push(SplitRow::Full(i));
                i += 1;
            }
        }
    }
    out
}
