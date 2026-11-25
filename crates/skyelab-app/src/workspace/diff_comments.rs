//! Comments on the Changes tab's diff lines (T3's `DiffCommentAnnotation`): what each one covers, finding its lines
//! again after a reload, and the chip text they go to the composer as. Kept per thread, in memory only.

use super::changes_model::{Changes, LineKind, Row, Scope};
use std::ops::Range;
use std::path::PathBuf;

#[cfg(test)]
#[path = "diff_comments_clip_test.rs"]
mod clip_test;
#[cfg(test)]
#[path = "diff_comments_test.rs"]
mod tests;

/// One commented line: what a reload matches on (kind and text) and what the label names (its numbers).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub kind: LineKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffComment {
    pub id: u64,
    /// The diff it was made on: a turn's diff has other rows than the working tree's.
    pub scope: Scope,
    /// Repo-relative, as the diff lists it.
    pub path: PathBuf,
    pub lines: Vec<Line>,
    pub note: String,
    /// The rows it covers in the diff shown now.
    pub rows: Range<usize>,
    /// Where its first row sits in its file, so a reload that grows an earlier file doesn't move it.
    offset: usize,
}

impl DiffComment {
    /// `src/a.rs:11-12`, by the new file's numbers; a comment on removed lines only names the old ones.
    pub(crate) fn label(&self) -> String {
        let new: Vec<u32> = self.lines.iter().filter_map(|l| l.new).collect();
        let (nums, old) = if new.is_empty() {
            (self.lines.iter().filter_map(|l| l.old).collect(), " (old)")
        } else {
            (new, "")
        };
        let range = match (nums.iter().min(), nums.iter().max()) {
            (Some(a), Some(b)) if a != b => format!(":{a}-{b}"),
            (Some(a), _) => format!(":{a}"),
            _ => String::new(),
        };
        format!("{}{range}{old}", self.path.display())
    }
}

impl DiffComment {
    /// The comment as a line comment of a GitHub review: from its first line to its last, each on the old side when
    /// it is a removed line and on the new side otherwise. `None` when a line has no number on its side.
    pub(crate) fn review_comment(&self) -> Option<skyelab_git::ReviewComment> {
        use skyelab_git::Side;
        let at = |l: &Line| match l.kind {
            LineKind::Removed => Some((l.old?, Side::Left)),
            _ => Some((l.new?, Side::Right)),
        };
        let (first, last) = (at(self.lines.first()?)?, at(self.lines.last()?)?);
        Some(skyelab_git::ReviewComment {
            path: self.path.to_string_lossy().into_owned(),
            line: last.0,
            side: last.1,
            start: Some(first),
            body: self.note.clone(),
        })
    }
}

/// A comment's excerpt shows at most this many lines in the chip, so its note always fits.
const MAX_EXCERPT: usize = 30;

/// A thread's diff comments.
#[derive(Debug, Default)]
pub(crate) struct DiffComments {
    pub items: Vec<DiffComment>,
    next: u64,
    /// How many reloads dropped since the user last added or removed one.
    dropped: usize,
}

/// The file `row` belongs to: its index in `changes.files` and its rows.
fn file_of(changes: &Changes, row: usize) -> Option<(usize, Range<usize>)> {
    let i = changes.files.iter().rposition(|f| f.first_row <= row)?;
    let end = changes
        .files
        .get(i + 1)
        .map_or(changes.rows.len(), |f| f.first_row);
    Some((i, changes.files[i].first_row..end))
}

/// The line rows in `rows`, with their indices.
fn lines(changes: &Changes, rows: Range<usize>) -> Vec<(usize, Line)> {
    rows.filter_map(|ix| match changes.rows.get(ix)? {
        Row::Line {
            old,
            new,
            kind,
            text,
        } => Some((
            ix,
            Line {
                kind: *kind,
                old: *old,
                new: *new,
                text: text.clone(),
            },
        )),
        _ => None,
    })
    .collect()
}

impl DiffComments {
    /// Comments `note` on the lines of rows `from..=to` (either order), kept inside `from`'s file. `None` when they
    /// hold no lines.
    pub(crate) fn add(
        &mut self,
        changes: &Changes,
        scope: Scope,
        from: usize,
        to: usize,
        note: &str,
    ) -> Option<u64> {
        let (file, rows) = file_of(changes, from)?;
        let (start, end) = (from.min(to).max(rows.start), from.max(to).min(rows.end - 1));
        let picked = lines(changes, start..end + 1);
        let (first, last) = (picked.first()?.0, picked.last()?.0);
        let id = self.next;
        self.next += 1;
        self.dropped = 0;
        self.items.push(DiffComment {
            id,
            scope,
            path: changes.files[file].path.clone(),
            lines: picked.into_iter().map(|(_, l)| l).collect(),
            note: note.trim().to_owned(),
            rows: first..last + 1,
            offset: first - rows.start,
        });
        Some(id)
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.items.retain(|c| c.id != id);
        self.dropped = 0;
    }

    /// Drops every `scope` comment (sent, or cleared).
    pub(crate) fn clear(&mut self, scope: Scope) {
        self.items.retain(|c| c.scope != scope);
        self.dropped = 0;
    }

    pub(crate) fn count(&self, scope: Scope) -> usize {
        self.items.iter().filter(|c| c.scope == scope).count()
    }

    /// Finds each `scope` comment's lines in a reloaded diff: the same file, the same kinds and texts in a row,
    /// nearest to where they were in that file. One whose lines are gone is dropped and counted.
    pub(crate) fn reanchor(&mut self, changes: &Changes, scope: Scope) {
        let before = self.items.len();
        self.items.retain_mut(|c| {
            if c.scope != scope {
                return true;
            }
            let Some(file) = changes.files.iter().position(|f| f.path == c.path) else {
                return false;
            };
            let Some((_, rows)) = file_of(changes, changes.files[file].first_row) else {
                return false;
            };
            let first_row = rows.start;
            let found = lines(changes, rows);
            let n = c.lines.len();
            if n == 0 {
                return false;
            }
            let same = |w: &[(usize, Line)]| {
                w.iter()
                    .zip(&c.lines)
                    .all(|((_, a), b)| a.kind == b.kind && a.text == b.text)
            };
            let best = found
                .windows(n)
                .filter(|w| same(w))
                .min_by_key(|w| (w[0].0 - first_row).abs_diff(c.offset));
            match best {
                Some(w) => {
                    c.rows = w[0].0..w[n - 1].0 + 1;
                    c.offset = w[0].0 - first_row;
                    c.lines = w.iter().map(|(_, l)| l.clone()).collect();
                    true
                }
                None => false,
            }
        });
        self.dropped += before - self.items.len();
    }

    /// "2 comments were dropped: …" after a reload lost some.
    pub(crate) fn dropped_note(&self) -> Option<String> {
        match self.dropped {
            0 => None,
            1 => Some("1 comment was dropped: its lines are no longer in the diff.".into()),
            n => Some(format!(
                "{n} comments were dropped: their lines are no longer in the diff."
            )),
        }
    }

    /// The chip's text: each `scope` comment's file and lines, the excerpt with its signs (its first [`MAX_EXCERPT`]
    /// lines), and the note. `None` when it is too long for one chip, which would cut notes off.
    pub(crate) fn chip_text(&self, scope: Scope) -> Option<String> {
        let text = self
            .items
            .iter()
            .filter(|c| c.scope == scope)
            .map(|c| {
                let mut s = c.label();
                for l in c.lines.iter().take(MAX_EXCERPT) {
                    let sign = match l.kind {
                        LineKind::Added => '+',
                        LineKind::Removed => '-',
                        LineKind::Context => ' ',
                    };
                    s.push_str(&format!("\n{sign} {}", l.text));
                }
                if let Some(more) = c.lines.len().checked_sub(MAX_EXCERPT).filter(|&n| n > 0) {
                    s.push_str(&format!("\n… {more} more lines"));
                }
                s.push_str(&format!("\nComment: {}", c.note));
                s
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        (text.chars().count() <= skyelab_core::terminal_context::MAX_CHARS).then_some(text)
    }
}
