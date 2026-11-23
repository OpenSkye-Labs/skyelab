//! The running-children cap, held on every start, resume and follow-up of a delegated child, whoever sent the
//! prompt (the parent agent, another device, the user). Each check runs under the store lock together with the
//! status write that takes the slot, so two launches can't both take the last one.

use anyhow::{Result, anyhow};
use skyelab_core::delegation::{check_children, is_busy};
use skyelab_store::Store;

/// Fails if `parent` already has the most busy children allowed, not counting `me` (the thread asking, if it
/// has a row yet). The caller writes `me`'s busy status before letting go of the store lock.
pub(super) fn claim(store: &Store, parent: i64, me: Option<i64>) -> Result<()> {
    check_children(busy(store, parent, me)?)
        .map_err(|why| anyhow!("the thread that delegated this one: {why}"))
}

/// `parent`'s children holding a slot, not counting `except`.
pub(super) fn busy(store: &Store, parent: i64, except: Option<i64>) -> Result<usize> {
    Ok(store
        .child_threads(parent)?
        .iter()
        .filter(|t| Some(t.id) != except && is_busy(&t.status))
        .count())
}
