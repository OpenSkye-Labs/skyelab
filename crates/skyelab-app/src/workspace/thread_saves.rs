//! Records the store refused, held in order until a retry saves them. One queue per thread view, shared by every
//! pump the view starts and by its follow-ups, so a stopped session's held events, a resume, and a follow-up prompt
//! all land in the store in the order they happened.

use super::{Record, SharedStore, ThreadView, append, lock};
use anyhow::Result;
use gpui::{AppContext as _, AsyncApp, Context};
use skyelab_agent::Event;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Tries when the queue was empty (a store that just started failing): the first retry after [`SAVE_BACKOFF`],
/// doubling. A queue already behind gets one try per new record.
const FIRST_FAILURE_TRIES: u32 = 5;
const SAVE_BACKOFF: Duration = Duration::from_millis(50);
/// While records are held, how often they are retried with nothing else happening (an idle or ended thread).
const RETRY_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(super) struct Unsaved(Arc<Mutex<VecDeque<Held>>>);

struct Held {
    record: Record,
    /// The row is stored and only its follow-up write (session id, status) is left, so a retry won't store it twice.
    row_stored: bool,
}

impl Unsaved {
    fn queue(&self) -> MutexGuard<'_, VecDeque<Held>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()) // plain data, consistent after a panic
    }

    /// Queues `record` behind what is held and saves the queue front to back. Returns how many are still held.
    /// Blocking: run it off the UI thread.
    pub(super) fn push_and_save(
        &self,
        store: &SharedStore,
        id: i64,
        record: Option<Record>,
    ) -> (usize, Result<()>) {
        let mut queue = self.queue();
        if let Some(record) = record {
            queue.push_back(Held {
                record,
                row_stored: false,
            });
        }
        let saved = save_in_order(store, id, &mut queue);
        (queue.len(), saved)
    }

    /// Saves what is held, or says why it can't. Blocking: run it off the UI thread.
    pub(super) fn flush(&self, store: &SharedStore, id: i64) -> Result<()> {
        self.push_and_save(store, id, None).1
    }

    pub(super) fn is_empty(&self) -> bool {
        self.queue().is_empty()
    }
}

/// A held follow-up prompt also marks its turn running, so a held `Result` saved before it can't leave the thread idle.
fn save_in_order(store: &SharedStore, id: i64, queue: &mut VecDeque<Held>) -> Result<()> {
    let mut store = lock(store)?;
    while let Some(h) = queue.front_mut() {
        if !h.row_stored {
            append(&mut store, id, &h.record)?;
            h.row_stored = true;
        }
        match &h.record {
            Record::Agent(Event::Init { session_id, .. }) => {
                store.set_agent_session_id(id, Some(session_id))?
            }
            Record::Agent(Event::Result { .. }) => store.set_thread_status(id, "idle")?,
            Record::Prompt(_) => store.set_thread_status(id, "running")?,
            _ => {}
        }
        queue.pop_front();
    }
    Ok(())
}

/// [`Unsaved::push_and_save`] off the UI thread, retried with backoff when the queue was empty before.
pub(super) async fn save(
    unsaved: &Unsaved,
    store: &SharedStore,
    id: i64,
    record: Record,
    cx: &mut AsyncApp,
) -> usize {
    let (q, s) = (unsaved.clone(), store.clone());
    let (was_empty, (mut left, mut saved)) = cx
        .background_spawn(async move {
            let was_empty = q.is_empty();
            (was_empty, q.push_and_save(&s, id, Some(record)))
        })
        .await;
    let mut delay = SAVE_BACKOFF;
    for _ in 1..if was_empty { FIRST_FAILURE_TRIES } else { 1 } {
        if saved.is_ok() {
            break;
        }
        cx.background_executor().timer(delay).await;
        delay *= 2;
        let (q, s) = (unsaved.clone(), store.clone());
        (left, saved) = cx
            .background_spawn(async move { q.push_and_save(&s, id, None) })
            .await;
    }
    if let Err(e) = saved {
        eprintln!("skyelab: {left} events of thread {id} not saved yet: {e:#}");
    }
    left
}

impl ThreadView {
    /// After a save: show how many are held, and keep retrying them on a timer until none are.
    pub(super) fn saved(&mut self, left: usize, cx: &mut Context<Self>) {
        if self.unsaved_events != left {
            self.unsaved_events = left;
            cx.notify();
        }
        let Some(id) = self.thread.as_ref().map(|t| t.id) else {
            return;
        };
        if left == 0 || self.retrying_saves {
            return;
        }
        self.retrying_saves = true;
        let (unsaved, store) = (self.unsaved.clone(), self.backend.store.clone());
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(RETRY_EVERY).await;
                let (q, s) = (unsaved.clone(), store.clone());
                let (left, _) = cx
                    .background_spawn(async move { q.push_and_save(&s, id, None) })
                    .await;
                let gone = this
                    .update(cx, |this, cx| {
                        if left == 0 {
                            this.retrying_saves = false;
                        }
                        this.saved(left, cx);
                    })
                    .is_err();
                if gone || left == 0 {
                    return;
                }
            }
        })
        .detach();
    }
}
