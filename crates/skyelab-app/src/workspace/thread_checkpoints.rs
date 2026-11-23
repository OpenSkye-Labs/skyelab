//! A checkpoint around every agent turn (skyelab-git's hidden refs; the scheme is in docs/decisions.md), so Changes
//! can show what each turn did and rewind to before it. Best effort: a folder that isn't a repo, or a failed
//! snapshot, never stops a turn.

use skyelab_store::Thread;
use std::path::Path;

/// The thread's ref name component: its id plus creation time, so two data dirs on one repo never share one.
pub(super) fn key(thread: &Thread) -> String {
    format!("{}-{}", thread.id, thread.created_at)
}

/// Right before a turn: the last checkpoint is rewritten to the tree now (checkpoint 0 for a thread's first turn).
pub(super) fn before_turn(cwd: &Path, key: &str) {
    let last = latest(cwd, key).unwrap_or(0);
    if let Err(e) = skyelab_git::capture_checkpoint(cwd, key, last) {
        note(cwd, &e);
    }
}

/// At a turn's end: the next checkpoint, or none if the turn had no starting one.
pub(super) fn after_turn(cwd: &Path, key: &str) {
    let Some(last) = latest(cwd, key) else { return };
    if let Err(e) = skyelab_git::capture_checkpoint(cwd, key, last + 1) {
        note(cwd, &e);
    }
}

/// Drops a purged thread's checkpoints, so `git gc` can prune their snapshots. Blocking: run it off the UI thread.
/// A discarded worktree's folder is gone, but its refs live in the project's repo, so that is used instead.
pub(crate) fn forget(thread: &Thread, project: Option<&Path>) {
    let key = key(thread);
    let Err(e) = skyelab_git::delete_checkpoints(&thread.cwd, &key) else {
        return;
    };
    match project {
        Some(p) => {
            if let Err(e) = skyelab_git::delete_checkpoints(p, &key) {
                note(p, &e);
            }
        }
        None => note(&thread.cwd, &e),
    }
}

fn latest(cwd: &Path, key: &str) -> Option<u32> {
    skyelab_git::checkpoints(cwd, key)
        .ok()?
        .last()
        .map(|c| c.turn)
}

fn note(cwd: &Path, e: &skyelab_git::Error) {
    if !matches!(e, skyelab_git::Error::NotARepo(_)) {
        eprintln!("skyelab: no checkpoint for {}: {e}", cwd.display());
    }
}
