//! Records the store refused, held in order until a retry saves them. One queue per thread view, shared by every
//! pump the view starts and by its follow-ups, so a stopped session's held events, a resume, and a follow-up prompt
//! all land in the store in the order they happened.

use super::{Record, SharedStore, ThreadView, append, lock};
use anyhow::Result;
use gpui::{AppContext as _, AsyncApp, Context};
use skyelab_agent::Event;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// Tries when the queue was empty (a store that just started failing): the first retry after [`SAVE_BACKOFF`],
/// doubling. A queue already behind gets one try per new record.
const FIRST_FAILURE_TRIES: u32 = 5;
const SAVE_BACKOFF: Duration = Duration::from_millis(50);
/// While records are held, how often they are retried with nothing else happening (an idle or ended thread).
const RETRY_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub(super) struct Unsaved(Arc<Mutex<Inner>>);

/// A session's count of prompts still waiting for their `Result` (`ThreadView::turns`).
pub(super) type Turns = Arc<AtomicUsize>;

#[derive(Default)]
struct Inner {
    queue: VecDeque<Held>,
    /// A session's last turn ended: write `idle` if its count is still zero by then (a follow-up may have started one).
    /// Kept apart from the rows, so a row the store refuses never holds the thread busy and its child slot taken.
    idle: Option<Turns>,
    /// The session the view runs now: an idle from an older one never lands.
    current: Option<Turns>,
}

struct Held {
    record: Record,
    /// The row is stored and only its follow-up write (session id) is left, so a retry won't store it twice.
    row_stored: bool,
}

impl Unsaved {
    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()) // plain data, consistent after a panic
    }

    /// Queues `record` behind what is held (and `idle`, a session whose last turn just ended), then saves everything
    /// in order. Returns how many writes are still held. Blocking: run it off the UI thread.
    pub(super) fn push_and_save(
        &self,
        store: &SharedStore,
        id: i64,
        record: Option<Record>,
        idle: Option<Turns>,
    ) -> (usize, Result<()>) {
        let mut inner = self.inner();
        if let Some(record) = record {
            inner.queue.push_back(Held {
                record,
                row_stored: false,
            });
        }
        if idle.is_some() {
            inner.idle = idle;
        }
        let saved = save_in_order(store, id, &mut inner);
        (inner.queue.len() + usize::from(inner.idle.is_some()), saved)
    }

    /// Queues `record` behind what is held without saving: the next save writes it first.
    pub(super) fn hold(&self, record: Record) {
        self.inner().queue.push_back(Held {
            record,
            row_stored: false,
        });
    }

    /// `run` started a session counting its turns in `turns`.
    pub(super) fn set_session(&self, turns: &Turns) {
        self.inner().current = Some(turns.clone());
    }

    /// Saves what is held, or says why it can't. Blocking: run it off the UI thread.
    pub(super) fn flush(&self, store: &SharedStore, id: i64) -> Result<()> {
        self.push_and_save(store, id, None, None).1
    }

    fn is_empty(&self) -> bool {
        let inner = self.inner();
        inner.queue.is_empty() && inner.idle.is_none()
    }
}

fn save_in_order(store: &SharedStore, id: i64, inner: &mut Inner) -> Result<()> {
    let mut store = lock(store)?;
    if let Some(turns) = &inner.idle {
        // Only for the current session, with no turn started since, and only over "running": a Stop or an exit
        // written meanwhile stays.
        let current = inner
            .current
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, turns));
        if current
            && turns.load(Ordering::SeqCst) == 0
            && store.thread(id)?.is_some_and(|t| t.status == "running")
        {
            store.set_thread_status(id, "idle")?;
        }
        inner.idle = None;
    }
    while let Some(h) = inner.queue.front_mut() {
        if !h.row_stored {
            append(&mut store, id, &h.record)?;
            h.row_stored = true;
        }
        if let Record::Agent(Event::Init { session_id, .. }) = &h.record {
            store.set_agent_session_id(id, Some(session_id))?
        }
        inner.queue.pop_front();
    }
    Ok(())
}

/// Saves an agent event off the UI thread, retried with backoff when nothing was held before. A `Result` ends the
/// turns of the prompts it answers (`prompts`: two for a folded follow-up, none for a background wake) here, once,
/// whether or not its row saves: the count follows the agent, the store catches up.
pub(super) async fn save(
    unsaved: &Unsaved,
    store: &SharedStore,
    id: i64,
    record: Record,
    turns: &Turns,
    cx: &mut AsyncApp,
) -> usize {
    let idle = match &record {
        Record::Agent(Event::Result { prompts, .. }) if *prompts > 0 => {
            let answered = usize::try_from(*prompts).unwrap_or(usize::MAX);
            let before = turns
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                    Some(n.saturating_sub(answered))
                })
                .unwrap_or_default();
            (before <= answered).then(|| turns.clone())
        }
        _ => None,
    };
    let (q, s) = (unsaved.clone(), store.clone());
    let (was_empty, (mut left, mut saved)) = cx
        .background_spawn(async move {
            let was_empty = q.is_empty();
            (was_empty, q.push_and_save(&s, id, Some(record), idle))
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
            .background_spawn(async move { q.push_and_save(&s, id, None, None) })
            .await;
    }
    if let Err(e) = saved {
        eprintln!("skyelab: {left} writes of thread {id} not saved yet: {e:#}");
    }
    left
}

impl ThreadView {
    /// Text still streaming when its turn ends without the final message (Stop, a crash, an interrupt) is stored as a
    /// partial message, so a reload shows it. It is queued ahead of whatever is saved next (the turn's result).
    pub(super) fn keep_partial(&mut self, cx: &mut Context<Self>) {
        let Some(record) = crate::workspace::thread_model::partial(&self.items) else {
            return;
        };
        self.push(&record, cx);
        let Some(id) = self.thread.as_ref().map(|t| t.id) else {
            return;
        };
        self.unsaved.hold(record);
        let (q, s) = (self.unsaved.clone(), self.backend.store.clone());
        let saving = cx.background_spawn(async move { q.push_and_save(&s, id, None, None).0 });
        cx.spawn(async move |this, cx| {
            let left = saving.await;
            let _ = this.update(cx, |this, cx| this.saved(left, cx));
        })
        .detach();
    }

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
                    .background_spawn(async move { q.push_and_save(&s, id, None, None) })
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
