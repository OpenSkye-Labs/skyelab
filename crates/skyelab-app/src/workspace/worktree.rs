//! A git worktree per thread, and the checks that keep it on the project's account.
//!
//! Worktrees live inside the project (`<project>/.skyelab/worktrees/<slug>`), so the normal
//! resolver binds them to the project's account. Nothing here sets an account: these checks only
//! refuse when the worktree would resolve to anything but the account the thread records.

use anyhow::{Context as _, Result, bail};
use skyelab_core::AccountsConfig;
use skyelab_git::Status;
use skyelab_terminal::SpawnOptions;
use std::path::{Path, PathBuf};

/// The folder inside a project that holds SkyeLab's worktrees, ignored through `info/exclude`.
const DIR: &str = ".skyelab";

pub(super) struct Created {
    pub path: PathBuf,
    pub branch: String,
}

/// The title in lowercase ASCII words, plus a time-based suffix so two threads with the same
/// title get different branches.
pub(super) fn slug(title: &str, now_ms: u128) -> String {
    let mut s = String::new();
    for c in title.chars() {
        if s.len() >= 24 {
            break;
        }
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
        } else if !s.is_empty() && !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-');
    let mut n = now_ms % 36u128.pow(5);
    let mut suffix = String::new();
    for _ in 0..5 {
        let d = (n % 36) as u32;
        suffix.insert(
            0,
            char::from_digit(d, 36).expect("d < 36 is a base-36 digit"),
        );
        n /= 36;
    }
    if s.is_empty() {
        format!("thread-{suffix}")
    } else {
        format!("{s}-{suffix}")
    }
}

/// Creates a worktree of `project` on `skyelab/<slug>` from its HEAD, and returns it once it
/// resolves to `account_id`. Anything else is refused and the worktree removed again.
pub(super) fn create(
    config: &AccountsConfig,
    project: &Path,
    account_id: &str,
    title: &str,
) -> Result<Created> {
    let project = std::fs::canonicalize(project)
        .with_context(|| format!("resolving {}", project.display()))?;
    let root = std::fs::canonicalize(skyelab_git::repo_root(&project)?)?;
    // `worktree add` checks out the whole repo, so the agent would start at a root nobody bound.
    if root != project {
        bail!(
            "worktrees need the project to be its repo's root, and {} is inside {}",
            project.display(),
            root.display()
        );
    }
    let base = project.join(DIR);
    let parent = base.join("worktrees");
    for dir in [&base, &parent] {
        if std::fs::symlink_metadata(dir).is_ok_and(|m| !m.is_dir()) {
            bail!("{} is not a plain folder; remove it first", dir.display());
        }
    }
    std::fs::create_dir_all(&parent).with_context(|| format!("creating {}", parent.display()))?;
    skyelab_git::exclude_locally(&project, &format!("/{DIR}/"))?;

    let slug = slug(title, now_ms());
    let branch = format!("skyelab/{slug}");
    let dest = parent.join(&slug);
    let path = skyelab_git::create_worktree(&project, &branch, "HEAD", Some(&dest), false)?;
    if let Err(e) = check(config, &project, &path, account_id) {
        return Err(match skyelab_git::remove_worktree(&project, &path, true) {
            Ok(()) => e,
            Err(c) => e.context(format!("removing the refused worktree also failed: {c}")),
        });
    }
    Ok(Created { path, branch })
}

/// Stores a new thread's worktree on its row.
pub(super) fn record(
    store: &skyelab_store::Store,
    thread: &mut skyelab_store::Thread,
    worktree: Option<Created>,
) -> Result<()> {
    if let Some(w) = worktree {
        store.set_thread_worktree(thread.id, Some(&w.path), Some(&w.branch))?;
        (thread.worktree_path, thread.worktree_branch) = (Some(w.path), Some(w.branch));
    }
    Ok(())
}

/// The worktree sits under the project and resolves to the thread's account.
pub(super) fn check(
    config: &AccountsConfig,
    project: &Path,
    worktree: &Path,
    account_id: &str,
) -> Result<()> {
    let real = std::fs::canonicalize(worktree)?;
    if !real.starts_with(project) {
        bail!(
            "the worktree resolved to {}, outside the project",
            real.display()
        );
    }
    resolves_to(config, &real, account_id)
}

/// The one check behind every worktree launch: `dir` resolves to exactly the thread's account.
fn resolves_to(config: &AccountsConfig, dir: &Path, account_id: &str) -> Result<()> {
    match config.resolve(dir) {
        Some(a) if a.id == account_id => Ok(()),
        Some(a) => bail!(
            "{} is bound to {}, not this thread's account ({account_id})",
            dir.display(),
            a.label
        ),
        None => bail!(
            "{} isn't bound to this thread's account ({account_id})",
            dir.display()
        ),
    }
}

/// A terminal in a thread's worktree, under the thread's account, or an error if the folder
/// now resolves to any other account (rebound since the thread started).
pub(super) fn terminal_options(
    config: &AccountsConfig,
    cwd: &Path,
    account_id: &str,
) -> Result<SpawnOptions> {
    resolves_to(config, cwd, account_id)?;
    Ok(SpawnOptions {
        cwd: cwd.to_owned(),
        command: None,
        env: super::terminal_env(config, cwd)?,
        cols: 80,
        lines: 24,
    })
}

pub(super) enum Discard {
    Removed,
    /// Dirty: removing it loses these changes. Pass this snapshot back to confirm.
    NeedsConfirm(Status),
}

/// Removes the worktree at `path` (the branch stays). A dirty one is removed only when
/// `confirmed` is the exact status it has now, so a confirm can't drop changes it never showed.
pub(super) fn discard(path: &Path, confirmed: Option<&Status>) -> Result<Discard> {
    match std::fs::symlink_metadata(path) {
        // Deleted by hand: nothing left to lose, and git prunes its record of it on its own.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Discard::Removed),
        Err(e) => return Err(e.into()),
        // A link could lead into another repo's worktree, and removing would hit that one.
        Ok(m) if !m.is_dir() => bail!("{} is no longer a folder", path.display()),
        Ok(_) => {}
    }
    let main = skyelab_git::list_worktrees(path)?
        .into_iter()
        .next()
        .context("git listed no worktrees")?
        .path;
    // With ignored files: `.env` and local configs are what a discard would silently lose.
    let now = skyelab_git::status_with_ignored(path)?;
    if now.is_clean() {
        skyelab_git::remove_worktree(&main, path, false)?;
        return Ok(Discard::Removed);
    }
    if confirmed != Some(&now) {
        return Ok(Discard::NeedsConfirm(now));
    }
    skyelab_git::remove_worktree(&main, path, true)?;
    Ok(Discard::Removed)
}

/// Files a discard would lose, for the confirm prompt, sorted.
pub(super) fn changed_files(st: &Status) -> Vec<&Path> {
    let mut paths: Vec<&Path> = st
        .staged
        .iter()
        .chain(&st.unstaged)
        .map(|c| c.path.as_path())
        .chain(st.untracked.iter().map(PathBuf::as_path))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// The confirm prompt: what a discard deletes, ignored files (`.env`, local configs) named apart.
pub(super) fn discard_message(st: &Status) -> String {
    fn list(paths: &[&Path]) -> String {
        let shown: Vec<String> = paths
            .iter()
            .take(5)
            .map(|p| p.display().to_string())
            .collect();
        let mut s = shown.join(", ");
        if paths.len() > shown.len() {
            s.push_str(&format!(" and {} more", paths.len() - shown.len()));
        }
        s
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let files = changed_files(st);
    let mut parts = Vec::new();
    if !files.is_empty() {
        parts.push(format!(
            "{} uncommitted change{}: {}",
            files.len(),
            plural(files.len()),
            list(&files)
        ));
    }
    let ignored: Vec<&Path> = st.ignored.iter().map(PathBuf::as_path).collect();
    if !ignored.is_empty() {
        parts.push(format!(
            "{} ignored file{} git won't keep: {}",
            ignored.len(),
            plural(ignored.len()),
            list(&ignored)
        ));
    }
    format!("Discarding deletes {}", parts.join("; and "))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}
