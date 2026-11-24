//! The Add project page's path field (T3's in-palette folder browse): a typed path split into the folder to list and
//! the name being typed, that folder's subfolders with their git branch (read from `.git/HEAD`, no process), Tab
//! completion, and the checks for a new project's folder and a clone URL. Read only, except nothing here writes.

use anyhow::{Result, bail};
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

/// The most of a clone URL accepted.
pub const MAX_URL: usize = 2048;

/// A typed path as a folder to list and the start of a name in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browse {
    /// The folder listed, `~` expanded.
    pub dir: PathBuf,
    /// What follows the last separator: the name being typed.
    pub filter: String,
    /// The text before `filter`, as typed (`~/` stays `~/`), so a completion keeps the user's spelling.
    pub typed_dir: String,
}

/// One subfolder of the listed folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
    /// `Some(branch)` for a git repo (`None` inside when HEAD is detached).
    pub git: Option<Option<String>>,
}

fn is_sep(c: char) -> bool {
    c == '/' || c == MAIN_SEPARATOR
}

/// Whether `query` reads as a path: `~`, `~/…`, an absolute path, or (Windows) `C:\…`.
fn looks_like_path(query: &str) -> bool {
    query == "~"
        || query
            .strip_prefix('~')
            .is_some_and(|r| r.starts_with(is_sep))
        || query.starts_with('/')
        || (cfg!(windows) && Path::new(query).is_absolute())
}

/// `query` with a leading `~` replaced by `home`; `None` when it isn't a path, or names `~` with no home.
fn expand(query: &str, home: Option<&Path>) -> Option<PathBuf> {
    if !looks_like_path(query) {
        return None;
    }
    match query.strip_prefix('~') {
        Some(rest) => Some(home?.join(rest.trim_start_matches(is_sep))),
        None => Some(PathBuf::from(query)),
    }
}

/// `query` as a folder to browse, or `None` when it is not a path (the page then filters its rows instead).
pub fn parse(query: &str, home: Option<&Path>) -> Option<Browse> {
    // `~` alone browses home, as `~/` does.
    let query = if query == "~" { "~/" } else { query };
    expand(query, home)?;
    let cut = query.rfind(is_sep).map_or(0, |i| i + 1);
    let (typed_dir, filter) = query.split_at(cut);
    Some(Browse {
        dir: expand(typed_dir, home)?,
        filter: filter.to_owned(),
        typed_dir: typed_dir.to_owned(),
    })
}

/// The folder the whole query names (for "Add this folder"), without a trailing separator; `None` when not a path.
pub fn typed_path(query: &str, home: Option<&Path>) -> Option<PathBuf> {
    let trimmed = query.trim_end_matches(is_sep);
    let trimmed = if trimmed.is_empty() { query } else { trimmed };
    expand(trimmed, home)
}

/// The subfolders of `b.dir` whose names start with the filter (ignoring case), then those containing it, each
/// group sorted by name; hidden ones only when the filter starts with `.`. At most `limit`.
pub fn list(b: &Browse, limit: usize) -> Vec<Folder> {
    let Ok(entries) = std::fs::read_dir(&b.dir) else {
        return Vec::new();
    };
    let filter = b.filter.to_lowercase();
    let mut found: Vec<(bool, String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if name.starts_with('.') && !filter.starts_with('.') {
                return None;
            }
            let lower = name.to_lowercase();
            let starts = lower.starts_with(&filter);
            if !starts && !lower.contains(&filter) {
                return None;
            }
            // `metadata` follows a link: a linked folder is listed, as a file manager would.
            let path = e.path();
            std::fs::metadata(&path)
                .is_ok_and(|m| m.is_dir())
                .then_some((!starts, name, path))
        })
        .collect();
    found.sort_by_key(|a| (a.0, a.1.to_lowercase()));
    found
        .into_iter()
        .take(limit)
        .map(|(_, name, path)| Folder {
            git: git_info(&path),
            name,
            path,
        })
        .collect()
}

/// What Tab puts in the field for folder `name` of `b`: the typed folder, the name and a separator.
pub fn complete(b: &Browse, name: &str) -> String {
    format!("{}{name}{MAIN_SEPARATOR}", b.typed_dir)
}

/// The git dir of `dir`'s own checkout: `.git`, or the folder a worktree's `.git` file points at.
pub fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    let meta = std::fs::metadata(&dot).ok()?;
    if meta.is_dir() {
        return Some(dot);
    }
    let text = read_small(&dot)?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    Some(dir.join(target))
}

/// A short text file, read up to 64 KB (HEAD, config); `None` past that or when unreadable.
pub(crate) fn read_small(path: &Path) -> Option<String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).ok()?;
    let mut text = String::new();
    file.take(64 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

/// `Some(branch)` when `dir` is a git checkout (`Some(None)` when its HEAD is detached), else `None`.
pub fn git_info(dir: &Path) -> Option<Option<String>> {
    let head = read_small(&git_dir(dir)?.join("HEAD"))?;
    Some(
        head.trim()
            .strip_prefix("ref: refs/heads/")
            .map(str::to_owned),
    )
}

/// The folder a new project goes in: `input` as a path, else as a name inside `base`. Refuses an empty name, a name
/// that climbs out (`..`), a file, and a folder that already has something in it.
pub fn new_project_dir(input: &str, base: &Path, home: Option<&Path>) -> Result<PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        bail!("Type a name for the new project.");
    }
    let dir = match typed_path(input, home) {
        Some(dir) => dir,
        None => {
            if input.contains(is_sep) || input == "." || input == ".." {
                bail!("Type a plain name, or a full path starting with ~ or /.");
            }
            base.join(input)
        }
    };
    if dir
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("A project path can't contain `..`.");
    }
    match std::fs::metadata(&dir) {
        Ok(m) if !m.is_dir() => bail!("{} is a file.", dir.display()),
        Ok(_) if std::fs::read_dir(&dir).is_ok_and(|mut d| d.next().is_some()) => {
            bail!(
                "{} already has files in it. Add it as a folder instead.",
                dir.display()
            )
        }
        _ => Ok(dir),
    }
}

/// The folder name a clone of `url` gets. Only a plain `https://host/…/name` URL with no login in it and no `=`
/// (it would end git's `-c http.<url>.*` keys) is accepted: an ssh key or a token can't be bound to an account.
pub fn clone_name(url: &str) -> Result<String> {
    let url = url.trim();
    if url.len() > MAX_URL {
        bail!("That URL is too long.");
    }
    let Some(rest) = url.strip_prefix("https://") else {
        bail!(
            "Only https:// URLs can be cloned: the account's GitHub login answers there, an ssh key can't be bound."
        );
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.contains('@') {
        bail!("Leave the login out of the URL: the account's GitHub login is used.");
    }
    if url.contains('=') || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        bail!("That URL has characters a clone URL can't have.");
    }
    let name = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let name = name.strip_suffix(".git").unwrap_or(name);
    if name.is_empty() || name == "." || name == ".." || name.starts_with('-') {
        bail!("The URL names no repository.");
    }
    Ok(name.to_owned())
}

#[cfg(test)]
#[path = "add_project_test.rs"]
mod tests;
