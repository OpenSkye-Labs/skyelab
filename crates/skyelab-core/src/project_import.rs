//! Onboarding's Import projects step (T3's `projectImport.logic`): folders from the agents' own session history,
//! which ones are preselected, grouped by repository, and which account each defaults to.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const RECENT_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const MIN_THREADS: usize = 3;

/// A folder some account's agent sessions ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    /// The account offered for it: the one whose history has the most sessions there.
    pub account: String,
    /// Sessions over every account's history.
    pub threads: usize,
    /// The newest session's last change, Unix ms.
    pub last_active_ms: i64,
    pub git: bool,
    /// `owner/name` from its origin URL.
    pub repo: Option<String>,
}

impl Candidate {
    /// T3's preselection: a git repo with at least 3 sessions, the newest within 30 days.
    pub fn recent(&self, now_ms: i64) -> bool {
        self.git
            && self.threads >= MIN_THREADS
            && self.last_active_ms <= now_ms
            && now_ms - self.last_active_ms <= RECENT_MS
    }
}

/// Candidates of one repository (or one repo folder without an origin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub label: String,
    /// Indexes into the candidates, in their order.
    pub rows: Vec<usize>,
    pub last_active_ms: i64,
}

/// One folder from an account's history: where, how many sessions, the newest one's Unix ms.
pub type Seen = (PathBuf, usize, i64);

/// Each account's `(folder, sessions, newest ms)`, folded into one candidate per folder. A folder in several
/// histories counts every session and defaults to the account with the most (the newest on a tie). Sorted by path.
pub fn merge(per_account: Vec<(String, Vec<Seen>)>) -> Vec<Candidate> {
    let mut by_path: BTreeMap<PathBuf, (Candidate, usize, i64)> = BTreeMap::new();
    for (account, folders) in per_account {
        for (path, threads, last) in folders {
            let entry = by_path.entry(path.clone()).or_insert_with(|| {
                let c = Candidate {
                    path,
                    account: account.clone(),
                    threads: 0,
                    last_active_ms: last,
                    git: false,
                    repo: None,
                };
                (c, 0, i64::MIN)
            });
            let (c, best, best_at) = entry;
            c.threads += threads;
            c.last_active_ms = c.last_active_ms.max(last);
            if (threads, last) > (*best, *best_at) {
                (c.account, *best, *best_at) = (account.clone(), threads, last);
            }
        }
    }
    by_path.into_values().map(|(c, _, _)| c).collect()
}

/// Repositories, newest activity first (then by label), each holding its candidates; plain folders (not git) in
/// the second list.
pub fn group(candidates: &[Candidate]) -> (Vec<Group>, Vec<usize>) {
    let mut groups: Vec<Group> = Vec::new();
    let mut other = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if !c.git {
            other.push(i);
            continue;
        }
        let label = c.repo.clone().unwrap_or_else(|| {
            c.path.file_name().map_or_else(
                || c.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        });
        // A repo folder without an origin is its own group, keyed by its path.
        let found = c
            .repo
            .as_ref()
            .and_then(|r| groups.iter_mut().find(|g| &g.label == r));
        match found {
            Some(g) => {
                g.rows.push(i);
                g.last_active_ms = g.last_active_ms.max(c.last_active_ms);
            }
            None => groups.push(Group {
                label,
                rows: vec![i],
                last_active_ms: c.last_active_ms,
            }),
        }
    }
    groups.sort_by(|a, b| {
        b.last_active_ms
            .cmp(&a.last_active_ms)
            .then_with(|| a.label.cmp(&b.label))
    });
    (groups, other)
}

/// `owner/name` of a GitHub-style remote URL (https, `git@host:`, `ssh://`), lowercased; `None` for a local path.
pub fn repo_name(url: &str) -> Option<String> {
    let url = url.trim();
    let path = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("ssh://"))
        .or_else(|| url.strip_prefix("git://"))
    {
        rest.split_once('/')?.1
    } else {
        // scp-like `user@host:owner/name`.
        let (head, rest) = url.split_once(':')?;
        if !head.contains('@') {
            return None;
        }
        rest
    };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.rsplit('/');
    let (name, owner) = (parts.next()?, parts.next()?);
    (!name.is_empty() && !owner.is_empty()).then(|| format!("{owner}/{name}").to_lowercase())
}

/// The `url` of `[remote "origin"]` in `dir`'s git config (a worktree's common dir too), read as text.
pub fn origin_url(dir: &Path) -> Option<String> {
    let git = crate::add_project::git_dir(dir)?;
    let common = crate::add_project::read_small(&git.join("commondir"))
        .map(|c| git.join(c.trim()))
        .unwrap_or(git);
    let text = crate::add_project::read_small(&common.join("config"))?;
    let mut in_origin = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

#[cfg(test)]
#[path = "project_import_test.rs"]
mod tests;
