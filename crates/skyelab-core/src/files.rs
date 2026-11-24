//! A thread folder's files: the listing, content search and preview reads. A path that is asked for goes through
//! [`inside`], and walks never follow symlinks, so nothing outside the folder is listed, searched or shown.

use anyhow::{Context as _, Result, bail};
use grep_searcher::{BinaryDetection, SearcherBuilder, sinks::Lossy};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Text files larger than this show a note instead of their contents, and search skips them.
pub const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
/// Images larger than this show a note: decoding a huge one stalls the window.
pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// A matched line is cut to this many chars for its preview row.
pub const MAX_LINE_CHARS: usize = 240;

// Not SVG: its `<image href>` loads any file (or a network share) when drawn, so it previews as text.
const IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// `path` (relative to `root`, or absolute) with symlinks resolved, when that is `root` itself or under it. `None`
/// when it escapes (`..`, an absolute path elsewhere, a symlink pointing out) or doesn't exist.
pub fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let root_real = root.canonicalize().ok()?;
    // Checked by the text first, so an escaping path is never touched: resolving a network share or an automount
    // (`\\host\share`, `/net/host`) would reach the network.
    let climbs = path.components().any(|c| matches!(c, Component::ParentDir));
    let relative =
        !path.has_root() && !matches!(path.components().next(), Some(Component::Prefix(_)));
    let anchored = relative || path.starts_with(root) || path.starts_with(&root_real);
    if climbs || !anchored {
        return None;
    }
    let root = root_real;
    let full = root.join(path).canonicalize().ok()?;
    full.starts_with(&root).then_some(full)
}

fn walker(root: &Path) -> ignore::Walk {
    // .gitignore'd files are skipped, in a repo or not; dotfiles are kept (git tracks them) but `.git` itself isn't
    // walked. Symlinks aren't followed (the default).
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .sort_by_file_name(|a, b| a.cmp(b))
        .build()
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<_> = rel.iter().map(|p| p.to_string_lossy()).collect();
    Some(parts.join("/"))
}

/// The files under `root` that aren't ignored, relative and `/`-separated, at most `cap`: the picker's list for a
/// folder that isn't a git repo.
pub fn walk(root: &Path, cap: usize) -> Vec<String> {
    walker(root)
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| relative(root, e.path()))
        .take(cap)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based.
    pub line: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatches {
    /// Relative to the searched folder, `/`-separated.
    pub path: String,
    pub lines: Vec<LineMatch>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    pub files: Vec<FileMatches>,
    /// The search stopped at its cap: there may be more.
    pub truncated: bool,
}

/// Searches the files under `root` that aren't ignored for `query`, as literal text, case-insensitive unless it has
/// an uppercase letter. Stops after `cap` matching lines, or soon after `cancel` is set. Binary files and files over
/// [`MAX_TEXT_BYTES`] are skipped.
pub fn search(root: &Path, query: &str, cap: usize, cancel: &AtomicBool) -> Result<Found> {
    let mut found = Found::default();
    if query.is_empty() || cap == 0 {
        return Ok(found);
    }
    let matcher = grep_regex::RegexMatcherBuilder::new()
        .case_smart(true)
        .fixed_strings(true)
        .build(query)
        .context("bad search text")?;
    let mut searcher = SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(0))
        .line_number(true)
        .build();
    let mut count = 0;
    for entry in walker(root) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else { continue };
        let small = entry.metadata().is_ok_and(|m| m.len() <= MAX_TEXT_BYTES);
        if !entry.file_type().is_some_and(|t| t.is_file()) || !small {
            continue;
        }
        let Some(path) = relative(root, entry.path()) else {
            continue;
        };
        let left = cap + 1 - count; // one past the cap tells whether there are more
        let mut lines = Vec::new();
        let sink = Lossy(|line, text: &str| {
            let text = text.trim_end_matches(['\n', '\r']);
            lines.push(LineMatch {
                line: u32::try_from(line).unwrap_or(u32::MAX),
                text: text.chars().take(MAX_LINE_CHARS).collect(),
            });
            Ok(lines.len() < left && !cancel.load(Ordering::Relaxed))
        });
        // An unreadable file is skipped, like ripgrep does.
        let _ = searcher.search_path(&matcher, entry.path(), sink);
        count += lines.len();
        if count > cap {
            lines.pop();
            found.truncated = true;
        }
        if !lines.is_empty() {
            found.files.push(FileMatches { path, lines });
        }
        if found.truncated {
            break;
        }
    }
    Ok(found)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Preview {
    Text(String),
    /// The file, resolved; the view decodes it.
    Image(PathBuf),
    Binary,
    TooLarge(u64),
}

/// What the preview shows for `path` (relative to `root`, or absolute). An error when it isn't a file in `root`.
pub fn read_preview(root: &Path, path: &Path) -> Result<Preview> {
    let full = inside(root, path).context("Not a file in this thread's folder.")?;
    let meta = std::fs::metadata(&full).context("Can't read this file.")?;
    if !meta.is_file() {
        bail!("Not a file.");
    }
    let ext = full
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if ext.as_deref().is_some_and(|e| IMAGES.contains(&e)) {
        return Ok(if meta.len() > MAX_IMAGE_BYTES {
            Preview::TooLarge(meta.len())
        } else {
            Preview::Image(full)
        });
    }
    if meta.len() > MAX_TEXT_BYTES {
        return Ok(Preview::TooLarge(meta.len()));
    }
    let bytes = std::fs::read(&full).context("Can't read this file.")?;
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return Ok(Preview::Binary);
    }
    Ok(String::from_utf8(bytes).map_or(Preview::Binary, Preview::Text))
}

/// `C:\x` for Windows' canonical `\\?\C:\x`: agents record the plain form. Other paths (a UNC share, any Unix
/// path) come back as they are. Git needs it too: `worktree add \\?\C:\…` fails to create the folder.
pub fn without_verbatim(path: &Path) -> PathBuf {
    let plain = path
        .to_str()
        .and_then(|s| s.strip_prefix(r"\\?\"))
        .filter(|rest| {
            let b = rest.as_bytes();
            b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
        });
    plain.map_or_else(|| path.to_owned(), PathBuf::from)
}

#[cfg(test)]
#[test]
fn verbatim_drive_paths_lose_their_prefix_only() {
    assert_eq!(
        without_verbatim(Path::new(r"\\?\C:\w\app")),
        Path::new(r"C:\w\app")
    );
    assert_eq!(
        without_verbatim(Path::new(r"\\?\UNC\host\share")),
        Path::new(r"\\?\UNC\host\share")
    );
    assert_eq!(without_verbatim(Path::new("/w/app")), Path::new("/w/app"));
}
