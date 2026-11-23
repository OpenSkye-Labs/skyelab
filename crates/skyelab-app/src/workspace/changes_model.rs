//! What the Changes tab shows: every changed file in a folder's repo against `HEAD` (or a
//! worktree's base), flattened into fixed-height rows so a long diff renders through `uniform_list`.

use anyhow::Result;
use skyelab_git::{ChangeKind, Line};
use std::io::Read as _;
use std::path::{Path, PathBuf};

/// Untracked files come from the agent, so what is read of them for display is capped per file and in total.
const MAX_UNTRACKED_FILE: u64 = 1 << 20;
const MAX_UNTRACKED_TOTAL: u64 = 16 << 20;
/// git's hash of the empty tree: what a repo with no commits yet is compared against.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

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
    /// A file whose diff has too many changed lines to show; the count is in the row.
    TooLarge(usize),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Changes {
    pub files: Vec<ChangedFile>,
    pub rows: Vec<Row>,
}

/// Tracked changes from `diff HEAD` (staged and unstaged together), untracked files from `status`.
/// With a `base` commit, tracked changes are since that commit, commits included.
pub fn load(dir: &Path, base: Option<&str>) -> Result<Changes> {
    let root = skyelab_git::repo_root(dir)?;
    let mut entries: Vec<(ChangedFile, Vec<Row>)> = Vec::new();
    // What is left of the diff's line cap for untracked files, after the tracked ones.
    let mut lines_left = skyelab_git::MAX_TOTAL_LINES;
    let diffs = match base {
        Some(base) => skyelab_git::diff_merge_base(dir, base)?,
        None => diff_head(dir)?,
    };
    for d in diffs {
        let Some(path) = d.new_path.clone().or(d.old_path.clone()) else {
            continue;
        };
        let mut rows = Vec::new();
        if d.binary {
            rows.push(Row::Placeholder("Binary file"));
        }
        if let Some(n) = d.too_large {
            rows.push(Row::TooLarge(n));
        }
        for h in &d.hunks {
            lines_left = lines_left.saturating_sub(h.lines.len());
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
    let mut budget = MAX_UNTRACKED_TOTAL;
    for rel in skyelab_git::status(dir)?.untracked {
        let (binary, mut rows) = untracked_rows(&root.join(&rel), &mut budget);
        let n = rows.len();
        if n > skyelab_git::MAX_FILE_LINES || n > lines_left {
            rows = vec![Row::TooLarge(n)];
        } else {
            lines_left -= n;
        }
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

/// `diff HEAD`, or against the empty tree while the repo has no commit to compare with.
fn diff_head(dir: &Path) -> Result<Vec<skyelab_git::FileDiff>> {
    match skyelab_git::diff(dir, "HEAD") {
        Err(skyelab_git::Error::Git { stderr, .. })
            if stderr.contains("bad revision") || stderr.contains("unknown revision") =>
        {
            Ok(skyelab_git::diff(dir, EMPTY_TREE)?)
        }
        other => Ok(other?),
    }
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

/// Longest diff line shown. A row never wraps and is clipped by the pane anyway, but shaping a minified
/// megabyte-long line on every frame would stall scrolling.
const MAX_LINE_CHARS: usize = 2_000;

fn line_row(old: Option<u32>, new: Option<u32>, kind: LineKind, text: &str) -> Row {
    let text = match text.char_indices().nth(MAX_LINE_CHARS) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    };
    Row::Line {
        old,
        new,
        kind,
        text,
    }
}

/// An untracked file shown as all added lines. Never follows a link: it could point at any file
/// on disk, another account's credentials included.
fn untracked_rows(path: &Path, budget: &mut u64) -> (bool, Vec<Row>) {
    let placeholder = |p| (false, vec![Row::Placeholder(p)]);
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return placeholder("Can't read this file");
    };
    if !meta.is_file() {
        return placeholder("Not a regular file");
    }
    if meta.len() > MAX_UNTRACKED_FILE {
        return placeholder("File too large to show");
    }
    if meta.len() > *budget {
        return placeholder("Not shown: too many new files");
    }
    let mut bytes = Vec::new();
    // shortcut: a FIFO swapped in after the lstat blocks this open; open with O_NONBLOCK if that shows up.
    let read = std::fs::File::open(path).and_then(|f| {
        if !same_file(&meta, &f.metadata()?) {
            return Err(std::io::Error::other("the file changed while it was read"));
        }
        f.take(MAX_UNTRACKED_FILE).read_to_end(&mut bytes)
    });
    if read.is_err() {
        return placeholder("Can't read this file");
    }
    *budget = budget.saturating_sub(bytes.len() as u64);
    // The same probe git uses: a NUL in the first 8000 bytes means binary.
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

/// The file opened is the one checked with lstat, not a link swapped in between.
#[cfg(unix)]
fn same_file(checked: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    opened.is_file() && (checked.dev(), checked.ino()) == (opened.dev(), opened.ino())
}

#[cfg(not(unix))]
fn same_file(_: &std::fs::Metadata, opened: &std::fs::Metadata) -> bool {
    opened.is_file()
}
