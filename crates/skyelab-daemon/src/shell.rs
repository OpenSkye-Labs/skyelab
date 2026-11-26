//! The shell stream (protocol 1.7): the store's row hook feeds one worker thread, which re-reads the changed rows
//! and hands each subscriber the same numbered changes. Local (`skyelab-env`) and remote subscribers alike.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use skyelab_protocol::{ShellChange, ShellDelta, ShellSnapshot};
use skyelab_store::{RowChange, Store};

/// Writes this close together become one read: a streaming turn bumps its thread's row on every event.
const BATCH: Duration = Duration::from_millis(100);
/// Changes a subscriber may still have unread when a batch comes before it is dropped (its stream ends, and it
/// subscribes again). Checked per batch, not per change: one write can fan out to a project's every thread.
const BACKLOG: usize = 1024;

/// A snapshot and every change after it. `changes` ends when the subscriber fell behind, or the backend it came
/// from (or its store) was dropped: subscribe again for a new snapshot.
pub struct ShellFeed {
    pub snapshot: ShellSnapshot,
    pub changes: flume::Receiver<ShellChange>,
}

/// Re-reads changed rows into deltas. It takes the store's lock itself, so it can read files before it does.
pub(crate) type ReadRows = Box<dyn Fn(&Mutex<Store>, &[RowChange]) -> Vec<ShellDelta> + Send>;

#[derive(Default)]
struct Subs {
    /// The last change handed out.
    seq: u64,
    senders: Vec<flume::Sender<ShellChange>>,
}

pub(crate) struct Hub {
    subs: Arc<Mutex<Subs>>,
}

impl Hub {
    /// Hooks `store` and starts the worker. The worker holds neither the store nor the hub: it ends once either
    /// is dropped and the next row changes (or the store, holding the hook, is gone).
    pub(crate) fn start(store: &Arc<Mutex<Store>>, read: ReadRows) -> std::io::Result<Self> {
        let subs = Arc::new(Mutex::new(Subs::default()));
        let (tx, rx) = flume::unbounded();
        let (weak_store, weak_subs) = (Arc::downgrade(store), Arc::downgrade(&subs));
        std::thread::Builder::new()
            .name("skyelab-shell".into())
            .spawn(move || work(&rx, &weak_store, &weak_subs, &read))?;
        store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .on_rows_changed(move |change| tx.send(change).is_ok());
        Ok(Self { subs })
    }

    /// Registers a subscriber, then reads the snapshot: every change after the returned seq reaches it, and one
    /// that raced the read only re-applies a row it already has (a later write always sends a later change).
    pub(crate) fn subscribe(
        &self,
        snapshot: impl FnOnce() -> Result<ShellSnapshot, skyelab_protocol::RpcError>,
    ) -> Result<ShellFeed, skyelab_protocol::RpcError> {
        let (tx, changes) = flume::unbounded();
        let seq = {
            let mut subs = self.subs.lock().unwrap_or_else(PoisonError::into_inner);
            subs.senders.push(tx);
            subs.seq
        };
        Ok(ShellFeed {
            snapshot: ShellSnapshot { seq, ..snapshot()? },
            changes,
        })
    }
}

fn work(
    rx: &flume::Receiver<RowChange>,
    store: &Weak<Mutex<Store>>,
    subs: &Weak<Mutex<Subs>>,
    read: &ReadRows,
) {
    while let Ok(first) = rx.recv() {
        let until = Instant::now() + BATCH;
        let mut rows = vec![first];
        while let Ok(change) = rx.recv_deadline(until) {
            rows.push(change);
        }
        let mut seen = HashSet::new();
        rows.retain(|r| seen.insert(*r));
        let Some(subs) = subs.upgrade() else { return };
        if subs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .senders
            .is_empty()
        {
            continue; // nobody listens: a later subscriber's snapshot has these rows
        }
        let Some(store) = store.upgrade() else { return };
        let deltas = read(&store, &rows);
        drop(store);
        let mut subs = subs.lock().unwrap_or_else(PoisonError::into_inner);
        // Still behind by a whole backlog from earlier batches: drop it, so its stream ends and it resubscribes.
        subs.senders.retain(|s| s.len() <= BACKLOG);
        for delta in deltas {
            subs.seq += 1;
            let change = ShellChange {
                seq: subs.seq,
                delta,
            };
            subs.senders.retain(|s| s.send(change.clone()).is_ok());
        }
    }
}
