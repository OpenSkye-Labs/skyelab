//! The running-children cap, held on every start, resume and follow-up of a delegated child, whoever sent the
//! prompt (the parent agent, another device, the user). Each check runs under the store lock together with the
//! status write that takes the slot, so two launches can't both take the last one. A child another machine's agent
//! started counts against that caller's cap and its machine's cap here.

use anyhow::{Result, anyhow};
use skyelab_core::delegation::{check_children, is_busy};
use skyelab_core::delegation_remote::check_origin;
use skyelab_store::Store;

/// Who started a delegated thread: a local thread, or an agent on another machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ParentKey {
    Local(i64),
    Remote {
        node: String,
        store: String,
        parent: i64,
    },
}

/// The parent of thread `id` (whose row's `parent_thread_id` is `local`), if an agent started it here or elsewhere.
pub(super) fn delegated_parent(
    store: &Store,
    id: i64,
    local: Option<i64>,
) -> Result<Option<ParentKey>> {
    if let Some(parent) = local {
        return Ok(Some(ParentKey::Local(parent)));
    }
    Ok(store.delegated_from(id)?.as_ref().map(remote_key))
}

/// The key a remote caller's children count under.
pub(super) fn remote_key(o: &skyelab_store::DelegatedOrigin) -> ParentKey {
    ParentKey::Remote {
        node: o.origin_node.clone(),
        store: o.origin_store.clone(),
        parent: o.parent_thread_id,
    }
}

/// Fails if `key` already has the most busy children allowed (and, for a remote caller, its machine too), not
/// counting `me`. The caller writes `me`'s busy status before letting go of the store lock.
pub(super) fn claim_for(store: &Store, key: &ParentKey, me: Option<i64>) -> Result<()> {
    match key {
        ParentKey::Local(parent) => claim(store, *parent, me),
        ParentKey::Remote {
            node,
            store: origin,
            parent,
        } => {
            let busy = |rows: Vec<(i64, String)>| {
                rows.iter()
                    .filter(|(id, status)| Some(*id) != me && is_busy(status))
                    .count()
            };
            check_children(busy(
                store.delegated_statuses(node, Some((origin, *parent)))?,
            ))
            .map_err(|why| anyhow!("the thread that delegated this one: {why}"))?;
            check_origin(busy(store.delegated_statuses(node, None)?)).map_err(|why| anyhow!(why))
        }
    }
}

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
