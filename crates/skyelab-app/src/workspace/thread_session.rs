//! A thread's agent session once started: the event pump that stores and shows each event,
//! and stopping, interrupting and reaping the agent.

use super::{Interrupt, Record, SharedStore, Stop, ThreadView, ThreadViewEvent, append, lock};
use anyhow::{Context as _, Result};
use gpui::{AppContext as _, AsyncApp, Context, Window};
use skyelab_agent::{Event, Session};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Tries for a batch of held events that just started failing, the first retry after [`SAVE_BACKOFF`], doubling.
const SAVE_TRIES: u32 = 5;
const SAVE_BACKOFF: Duration = Duration::from_millis(50);

/// An agent event not fully stored yet. `appended` keeps a retry from storing the row twice when only its
/// follow-up write (session id, status) failed.
struct Held {
    record: Record,
    appended: bool,
}

/// Saves `held` front to back, up to `tries` times. What is still held afterwards stays for the next event.
async fn save_held(
    store: &SharedStore,
    id: i64,
    held: &mut VecDeque<Held>,
    tries: u32,
    cx: &mut AsyncApp,
) {
    let mut delay = SAVE_BACKOFF;
    for attempt in 1..=tries {
        let (store, batch) = (store.clone(), std::mem::take(held));
        let (rest, saved) = cx
            .background_spawn(async move { save_in_order(&store, id, batch) })
            .await;
        *held = rest;
        match saved {
            Ok(()) => return,
            Err(e) if attempt == tries => {
                eprintln!(
                    "skyelab: {} agent events of thread {id} not saved yet: {e:#}",
                    held.len()
                );
            }
            Err(_) => {
                cx.background_executor().timer(delay).await;
                delay *= 2;
            }
        }
    }
}

fn save_in_order(
    store: &SharedStore,
    id: i64,
    mut batch: VecDeque<Held>,
) -> (VecDeque<Held>, Result<()>) {
    let saved = (|| {
        let mut store = lock(store)?;
        while let Some(h) = batch.front_mut() {
            if !h.appended {
                append(&mut store, id, &h.record)?;
                h.appended = true;
            }
            match &h.record {
                Record::Agent(Event::Init { session_id, .. }) => {
                    store.set_agent_session_id(id, Some(session_id))?
                }
                Record::Agent(Event::Result { .. }) => store.set_thread_status(id, "idle")?,
                _ => {}
            }
            batch.pop_front();
        }
        anyhow::Ok(())
    })();
    (batch, saved)
}

impl ThreadView {
    /// Each event is stored before it is shown, so the store keeps stdout's order. An event the store refuses is
    /// still shown and held, with every later one, until a retry saves them in order: never dropped.
    pub(super) fn run(
        &mut self,
        session: Session,
        store: SharedStore,
        id: i64,
        cx: &mut Context<Self>,
    ) {
        let events = session.events().clone();
        self.session = Some(Arc::new(Mutex::new(session)));
        let launch = self.launches;
        // A stopped session's pump keeps storing what it already printed.
        if let Some(old) = self._pump.take() {
            old.detach();
        }
        self._pump = Some(cx.spawn(async move |this, cx| {
            let mut held = VecDeque::new();
            while let Ok(event) = events.recv_async().await {
                let record = Record::Agent(event);
                held.push_back(Held {
                    record: record.clone(),
                    appended: false,
                });
                // A first failure gets retries with backoff; while already behind, one try per event.
                let tries = if held.len() == 1 { SAVE_TRIES } else { 1 };
                save_held(&store, id, &mut held, tries, cx).await;
                let shown = this.update(cx, |this, cx| {
                    this.set_unsaved(held.len(), cx);
                    if let (Record::Agent(Event::Init { session_id, .. }), Some(t)) =
                        (&record, &mut this.thread)
                    {
                        t.agent_session_id = Some(session_id.clone());
                        if this.session.is_none() && !this.starting {
                            this.finish(cx); // stopped before this arrived: it's resumable now
                        }
                    }
                    this.push(&record, cx);
                    if matches!(record, Record::Agent(Event::Result { .. })) {
                        cx.emit(ThreadViewEvent::TurnDone);
                    }
                });
                if shown.is_err() {
                    return;
                }
            }
            if !held.is_empty() {
                save_held(&store, id, &mut held, SAVE_TRIES, cx).await;
            }
            let _ = this.update(cx, |this, cx| {
                this.set_unsaved(held.len(), cx);
                this.ended(launch, cx)
            });
        }));
    }

    fn set_unsaved(&mut self, n: usize, cx: &mut Context<Self>) {
        if self.unsaved_events != n {
            self.unsaved_events = n;
            cx.notify();
        }
    }

    /// claude's stdout closed and every event is stored: record how the thread ended.
    fn ended(&mut self, launch: u64, cx: &mut Context<Self>) {
        if launch != self.launches {
            return; // a stopped session's stream; Stop already recorded it
        }
        let Some(thread) = &self.thread else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        let status = if self.stop_requested {
            "stopped"
        } else {
            "exited"
        };
        let session = self.session.take();
        self.finish(cx);
        // Reaping the child waits, so it happens off the UI thread, after the status is written
        // (so a quick resume's "running" can't be overwritten by it).
        self.in_background(cx, move || {
            lock(&store)?.set_thread_status(id, status)?;
            drop(session);
            Ok(())
        });
    }

    /// The agent is gone. The next prompt resumes its session, if it reported one.
    pub(super) fn finish(&mut self, cx: &mut Context<Self>) {
        self.grant = None; // revokes the token and cuts the MCP connection
        self.read_only = self
            .thread
            .as_ref()
            .is_none_or(|t| t.agent_session_id.is_none());
        let disabled = self.read_only;
        self.prompt.update(cx, |p, cx| p.set_disabled(disabled, cx));
        cx.notify();
    }

    pub(in crate::workspace) fn interrupt(
        &mut self,
        _: &Interrupt,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.session.clone() {
            self.in_background(cx, move || lock(&session)?.interrupt());
        }
    }

    pub(in crate::workspace) fn stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_requested = true;
        // While starting, the start task sees the flag and kills the new session.
        if !self.starting {
            self.kill(cx);
        }
        cx.notify();
    }

    /// Kills the agent. The pump keeps storing what it already printed, then `ended` runs.
    pub(super) fn kill(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        self._task = Some(cx.spawn(async move |this, cx| {
            let stopped = cx
                .background_spawn(async move {
                    lock(&session)?.kill().context("stopping the agent")?;
                    lock(&store)?.set_thread_status(id, "stopped")
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = stopped {
                    return this.show_error(e, cx);
                }
                this.session = None; // killed and reaped, so dropping it doesn't wait
                this.finish(cx);
            });
        }));
    }

    /// Runs blocking work on the background executor and shows its error, if any, inline.
    pub(super) fn in_background(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) {
        let task = cx.background_spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                let _ = this.update(cx, |this, cx| this.show_error(e, cx));
            }
        })
        .detach();
    }
}

impl Drop for ThreadView {
    /// Closing the tab ends its agent. Killing and reaping waits, so it happens off the UI thread.
    fn drop(&mut self) {
        let (Some(session), Some(thread)) = (self.session.take(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        let _ = std::thread::Builder::new()
            .name("skyelab-agent-reaper".into())
            .spawn(move || {
                drop(session);
                if let Ok(store) = lock(&store) {
                    let _ = store.set_thread_status(id, "stopped");
                }
            });
    }
}
