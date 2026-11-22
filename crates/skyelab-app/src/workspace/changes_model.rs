//! What the Changes tab shows: every changed file in a folder's repo against `HEAD`, flattened
//! into fixed-height rows so a long diff renders through `uniform_list`.

use anyhow::Result;
use skyelab_git::{ChangeKind, Line};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Longest untracked file read for display; it comes from the repo, so it is untrusted.
const MAX_UNTRACKED: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFile {
    pub path: PathBuf,
    /// Source of a rename or copy.
    pub old_path: Option<PathBuf>,
    pub kind: ChangeKind,
    pub binary: bool,
    /// Index of this file's header in [`Changes::rows`].
    pub first_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// Index into [`Changes::files`].
    File(usize),
    Hunk(String),
    Line {
        old: Option<u32>,
        new: Option<u32>,
        kind: LineKind,
        text: String,
    },
    /// No lines to show (binary, too large, a symlink).
    Placeholder(&'static str),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub files: Vec<ChangedFile>,
    pub rows: Vec<Row>,
}

/// Tracked changes from `diff HEAD` (staged and unstaged together), untracked files from `status`.
pub fn load(dir: &Path) -> Result<Changes> {
    let root = skyelab_git::repo_root(dir)?;
    let mut entries: Vec<(ChangedFile, Vec<Row>)> = Vec::new();
    for d in skyelab_git::diff(dir, "HEAD")? {
        let Some(path) = d.new_path.clone().or(d.old_path.clone()) else {
            continue;
        };
        let mut rows = Vec::new();
        if d.binary {
            rows.push(Row::Placeholder("Binary file"));
        }
        for h in &d.hunks {
            rows.push(Row::Hunk(format!(
                "@@ -{},{} +{},{} @@ {}",
                h.old_start, h.old_lines, h.new_start, h.new_lines, h.header
            )));
            let (mut old, mut new) = (h.old_start, h.new_start);
            for line in &h.lines {
                rows.push(match line {
                    Line::Context(t) => {
                        let row = line_row(Some(old), Some(new), LineKind::Context, t);
                        (old, new) = (old + 1, new + 1);
                        row
                    }
                    Line::Removed(t) => {
                        old += 1;
                        line_row(Some(old - 1), None, LineKind::Removed, t)
                    }
                    Line::Added(t) => {
                        new += 1;
                        line_row(None, Some(new - 1), LineKind::Added, t)
                    }
                });
            }
        }
        let old_path = matches!(d.kind, ChangeKind::Renamed | ChangeKind::Copied)
            .then(|| d.old_path.clone())
            .flatten();
        entries.push((file(path, old_path, d.kind, d.binary), rows));
    }
    for rel in skyelab_git::status(dir)?.untracked {
        let (binary, rows) = untracked_rows(&root.join(&rel));
        entries.push((file(rel, None, ChangeKind::Added, binary), rows));
    }
    entries.sort_by(|a, b| a.0.path.cmp(&b.0.path));

    let mut changes = Changes::default();
    for (mut f, rows) in entries {
        f.first_row = changes.rows.len();
        changes.rows.push(Row::File(changes.files.len()));
        changes.rows.extend(rows);
        changes.files.push(f);
    }
    Ok(changes)
}

fn file(path: PathBuf, old_path: Option<PathBuf>, kind: ChangeKind, binary: bool) -> ChangedFile {
    ChangedFile {
        path,
        old_path,
        kind,
        binary,
        first_row: 0,
    }
}

fn line_row(old: Option<u32>, new: Option<u32>, kind: LineKind, text: &str) -> Row {
    Row::Line {
        old,
        new,
        kind,
        text: text.to_owned(),
    }
}

/// An untracked file shown as all added lines. Never follows a link: it could point at any file
/// on disk, another account's credentials included.
fn untracked_rows(path: &Path) -> (bool, Vec<Row>) {
    let placeholder = |p| (false, vec![Row::Placeholder(p)]);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return placeholder("Can't read this file");
    };
    if !meta.is_file() {
        return placeholder("Not a regular file");
    }
    if meta.len() > MAX_UNTRACKED {
        return placeholder("File too large to show");
    }
    let mut bytes = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|f| f.take(MAX_UNTRACKED).read_to_end(&mut bytes));
    if read.is_err() {
        return placeholder("Can't read this file");
    }
    // git's own test: a NUL in the first 8000 bytes means binary.
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return (true, vec![Row::Placeholder("Binary file")]);
    }
    let text = String::from_utf8_lossy(&bytes);
    let rows = text
        .lines()
        .enumerate()
        .map(|(i, l)| line_row(None, Some(i as u32 + 1), LineKind::Added, l))
        .collect();
    (false, rows)
}
