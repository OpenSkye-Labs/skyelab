use std::path::{Path, PathBuf};

use crate::{ChangeKind, Result, git, lossy};

/// One file in a diff. Paths are relative to the repo root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// `None` for an added file.
    pub old_path: Option<PathBuf>,
    /// `None` for a deleted file.
    pub new_path: Option<PathBuf>,
    pub kind: ChangeKind,
    /// Binary files carry no hunks.
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    /// Text after the closing `@@`, usually the enclosing function.
    pub header: String,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Context(String),
    Added(String),
    Removed(String),
}

/// The working tree at `path` against `against` (a commit-ish such as `HEAD` or `main`), per file.
/// Untracked files are not included; see [`crate::status`] for those.
pub fn diff(path: &Path, against: &str) -> Result<Vec<FileDiff>> {
    let out = git(
        path,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-relative",
            "-M",
            // Explicit prefixes beat a user's diff.noprefix / diff.mnemonicPrefix, which the parser relies on.
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--end-of-options",
            against,
            "--",
        ],
    )?;
    Ok(parse(&out))
}

fn parse(out: &[u8]) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    // Header lines (---, rename from, …) only count before a file's first hunk: inside one,
    // "--- x" is a removed line whose text starts with "-- ".
    let mut in_hunk = false;
    for raw in out.split(|b| *b == b'\n') {
        let line = lossy(raw);
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (old, new) = header_paths(rest);
            files.push(FileDiff {
                old_path: old,
                new_path: new,
                kind: ChangeKind::Modified,
                binary: false,
                hunks: Vec::new(),
            });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(hunk) = parse_hunk_header(&line) {
            file.hunks.push(hunk);
            in_hunk = true;
        } else if in_hunk {
            let Some(hunk) = file.hunks.last_mut() else {
                continue;
            };
            // "\ No newline at end of file" and the trailing empty split are skipped.
            match line.split_at_checked(1) {
                Some((" ", text)) => hunk.lines.push(Line::Context(text.into())),
                Some(("+", text)) => hunk.lines.push(Line::Added(text.into())),
                Some(("-", text)) => hunk.lines.push(Line::Removed(text.into())),
                _ => {}
            }
        } else if line.starts_with("new file mode") {
            file.kind = ChangeKind::Added;
            file.old_path = None;
        } else if line.starts_with("deleted file mode") {
            file.kind = ChangeKind::Deleted;
            file.new_path = None;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            file.kind = ChangeKind::Renamed;
            file.old_path = Some(unquote(p).into());
        } else if let Some(p) = line.strip_prefix("rename to ") {
            file.new_path = Some(unquote(p).into());
        } else if let Some(p) = line.strip_prefix("copy from ") {
            file.kind = ChangeKind::Copied;
            file.old_path = Some(unquote(p).into());
        } else if let Some(p) = line.strip_prefix("copy to ") {
            file.new_path = Some(unquote(p).into());
        } else if let Some(p) = line.strip_prefix("--- ") {
            file.old_path = marker_path(p, "a/");
        } else if let Some(p) = line.strip_prefix("+++ ") {
            file.new_path = marker_path(p, "b/");
        } else if line.starts_with("Binary files ") {
            file.binary = true;
        }
    }
    files
}

/// Paths from `diff --git a/X b/Y`. Unambiguous only when X == Y, which is every case without a
/// rename or copy, and those carry `rename from/to` lines that overwrite this. Splitting at the
/// middle is how `git apply` reads the same line.
fn header_paths(rest: &str) -> (Option<PathBuf>, Option<PathBuf>) {
    let mid = rest.len() / 2;
    let (Some(a), Some(b)) = (
        rest.get(..mid),
        rest.get(mid..).and_then(|b| b.strip_prefix(' ')),
    ) else {
        return (None, None);
    };
    let (a, b) = (unquote(a), unquote(b));
    (
        a.strip_prefix("a/").map(PathBuf::from),
        b.strip_prefix("b/").map(PathBuf::from),
    )
}

/// A `---`/`+++` path: `/dev/null` means absent. Git adds a trailing tab when the name has a space.
fn marker_path(p: &str, prefix: &str) -> Option<PathBuf> {
    let p = p.strip_suffix('\t').unwrap_or(p);
    if p == "/dev/null" {
        return None;
    }
    let p = unquote(p);
    Some(p.strip_prefix(prefix).unwrap_or(&p).into())
}

/// `@@ -a,b +c,d @@ header`; a missing count means 1.
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, header) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((s, n)) => Some((s.parse().ok()?, n.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let ((old_start, old_lines), (new_start, new_lines)) = (range(old)?, range(new)?);
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        header: header.trim_start().into(),
        lines: Vec::new(),
    })
}

/// Undoes git's C-style quoting (`"a\tb\303\274"`); an unquoted name comes back unchanged.
pub(crate) fn unquote(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_owned();
    };
    let mut out = Vec::with_capacity(inner.len());
    let mut bytes = inner.bytes();
    while let Some(b) = bytes.next() {
        if b != b'\\' {
            out.push(b);
            continue;
        }
        let Some(e) = bytes.next() else { break };
        out.push(match e {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'v' => 0x0b,
            b'0'..=b'7' => {
                // Three octal digits: one raw byte of a UTF-8 sequence.
                let d = |c: Option<u8>| c.filter(|c| (b'0'..=b'7').contains(c)).map(|c| c - b'0');
                let (Some(d2), Some(d3)) = (d(bytes.next()), d(bytes.next())) else {
                    break;
                };
                ((e - b'0') << 6) | (d2 << 3) | d3
            }
            other => other, // \\ and \"
        });
    }
    lossy(&out)
}
