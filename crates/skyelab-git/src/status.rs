use std::path::{Path, PathBuf};

use crate::{ChangeKind, Result, git, lossy};

/// Working-tree state. Paths are relative to the repo root, whatever subdir `status` was called on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// Index vs `HEAD`.
    pub staged: Vec<FileChange>,
    /// Working tree vs index. Merge conflicts land here as [`ChangeKind::Unmerged`].
    pub unstaged: Vec<FileChange>,
    pub untracked: Vec<PathBuf>,
    /// Ignored files, and folders ignored as a whole (with a trailing `/`). Only
    /// [`status_with_ignored`] fills this.
    pub ignored: Vec<PathBuf>,
}

impl Status {
    /// Every path with a change to commit: staged, unstaged or untracked, each once.
    pub fn changed_paths(&self) -> std::collections::BTreeSet<&Path> {
        self.staged
            .iter()
            .chain(&self.unstaged)
            .map(|f| f.path.as_path())
            .chain(self.untracked.iter().map(PathBuf::as_path))
            .collect()
    }

    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.ignored.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    /// Source path of a rename or copy.
    pub orig_path: Option<PathBuf>,
    pub kind: ChangeKind,
}

pub fn status(path: &Path) -> Result<Status> {
    run(path, "--ignored=no")
}

/// [`status`] plus ignored files: what removing the worktree would also delete. A folder ignored
/// as a whole (`node_modules/`) is one entry, not every file in it.
pub fn status_with_ignored(path: &Path) -> Result<Status> {
    run(path, "--ignored=matching")
}

fn run(path: &Path, ignored: &str) -> Result<Status> {
    let out = git(
        path,
        &[
            "-c",
            "status.relativePaths=false",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            ignored,
            // Explicit, so a user's status.renames=false can't split a rename into add + delete.
            "--renames",
            // A submodule's dirty check runs git inside it, under its own config, which no override reaches.
            "--ignore-submodules=dirty",
        ],
    )?;
    Ok(parse(&out))
}

/// Parses `status --porcelain=v2 -z`. Ordinary entries are `1 XY sub mH mI mW hH hI path`, renames
/// `2 XY sub mH mI mW hH hI score path` followed by a separate NUL field with the original path.
fn parse(out: &[u8]) -> Status {
    let mut st = Status::default();
    let mut fields = out.split(|b| *b == 0).map(lossy);
    while let Some(entry) = fields.next() {
        let (tag, rest) = entry.split_once(' ').unwrap_or((&entry, ""));
        match tag {
            "?" => st.untracked.push(rest.into()),
            "!" => st.ignored.push(rest.into()),
            "1" | "2" | "u" => {
                let skip = match tag {
                    "1" => 7,
                    "2" => 8,
                    _ => 9,
                };
                // splitn keeps spaces inside the path, which is always the last field.
                let parts: Vec<&str> = rest.splitn(skip + 1, ' ').collect();
                let (Some(xy), Some(path)) = (parts.first(), parts.get(skip)) else {
                    continue;
                };
                let orig_path = (tag == "2")
                    .then(|| fields.next().map(PathBuf::from))
                    .flatten();
                let mut xy = xy.chars();
                let (x, y) = (xy.next().unwrap_or('.'), xy.next().unwrap_or('.'));
                let change = |kind| FileChange {
                    path: PathBuf::from(path),
                    orig_path: orig_path.clone(),
                    kind,
                };
                if tag == "u" {
                    st.unstaged.push(change(ChangeKind::Unmerged));
                    continue;
                }
                if let Some(kind) = kind(x) {
                    st.staged.push(change(kind));
                }
                if let Some(kind) = kind(y) {
                    // The source belongs to whichever side did the rename (`.R` happens for intent-to-add files).
                    let renamed_here = matches!(y, 'R' | 'C');
                    st.unstaged.push(FileChange {
                        orig_path: if renamed_here {
                            orig_path.clone()
                        } else {
                            None
                        },
                        ..change(kind)
                    });
                }
            }
            _ => {}
        }
    }
    st
}

fn kind(c: char) -> Option<ChangeKind> {
    Some(match c {
        'A' => ChangeKind::Added,
        'M' => ChangeKind::Modified,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'T' => ChangeKind::TypeChanged,
        _ => return None,
    })
}
