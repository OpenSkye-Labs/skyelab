//! A thread's agent session once started: the event pump that stores and shows each event,
//! and stopping, interrupting and reaping the agent.

use super::{Interrupt, Record, SharedStore, Stop, ThreadView, ThreadViewEvent, append, lock};
use anyhow::{Context as _, Result};
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::{Event, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

impl ThreadView {
    /// Each event is stored before it is shown, so the store keeps stdout's order.
    pub(super) fn run(
        &mut self,
        session: Session,
        store: SharedStore,
        id: i64,
        cx: &mut Context<Self>,
    ) {
        let events = session.events().clone();
        self.session = Some(Arc::new(Mutex::new(session)));
        // The launch sent one prompt.
        let turns = Arc::new(AtomicUsize::new(1));
        self.turns = turns.clone();
        let launch = self.launches;
        // A stopped session's pump keeps storing what it already printed.
        if let Some(old) = self._pump.take() {
            old.detach();
        }
        self._pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv_async().await {
                let record = Record::Agent(event);
                let store = store.clone();
                let saved_record = record.clone();
                let turns = turns.clone();
                let saved = cx
                    .background_spawn(async move {
                        let mut store = lock(&store)?;
                        append(&mut store, id, &saved_record)?;
                        match &saved_record {
                            Record::Agent(Event::Init { session_id, .. }) => {
                                store.set_agent_session_id(id, Some(session_id))?
                            }
                            Record::Agent(Event::Result { .. }) => {
                                let before = turns
                                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                        Some(n.saturating_sub(1))
                                    })
                                    .unwrap_or_default();
                                if before <= 1 {
                                    store.set_thread_status(id, "idle")?
                                }
                            }
                            _ => {}
                        }
                        anyhow::Ok(())
                    })
                    .await;
                let shown = this.update(cx, |this, cx| {
                    if let Err(e) = saved {
                        this.show_error(e.context("saving an agent event"), cx);
                    }
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
            let _ = this.update(cx, |this, cx| this.ended(launch, cx));
        }));
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

impl ThreadView {
    /// The app is quitting: end the agent and record the thread stopped before the process goes.
    pub(super) fn quitting(&mut self) -> impl std::future::Future<Output = ()> + use<> {
        let job = self.session.take().zip(self.thread.as_ref().map(|t| t.id));
        let store = self.backend.store.clone();
        async move {
            let Some((session, id)) = job else { return };
            if let Ok(store) = lock(&store) {
                let _ = store.set_thread_status(id, "stopped");
            }
            if let Ok(mut session) = lock(&session) {
                let _ = session.kill();
            }
        }
    }
}

/// Drops `session` (which kills and reaps its agent, and waits) off the UI thread, then records `id` stopped.
fn stop_in_background(session: impl Send + 'static, store: SharedStore, id: i64) {
    let _ = std::thread::Builder::new()
        .name("skyelab-agent-reaper".into())
        .spawn(move || {
            drop(session);
            if let Ok(store) = lock(&store) {
                let _ = store.set_thread_status(id, "stopped");
            }
        });
}

/// A launch's new agent until its view takes it. Dropped untaken (the tab closed while it started), it ends the
/// agent and records the thread stopped, so the row never says running with nothing behind it.
pub(super) struct NewSession {
    session: Option<Session>,
    store: SharedStore,
    id: i64,
}

impl NewSession {
    pub(super) fn new(session: Session, store: SharedStore, id: i64) -> Self {
        Self {
            session: Some(session),
            store,
            id,
        }
    }

    pub(super) fn take(mut self) -> Option<Session> {
        self.session.take()
    }
}

impl Drop for NewSession {
    fn drop(&mut self) {
        if let Some(session) = self.session.take() {
            stop_in_background(session, self.store.clone(), self.id);
        }
    }
}

impl Drop for ThreadView {
    /// Closing the tab ends its agent. A launch still in progress ends its own (`NewSession`).
    fn drop(&mut self) {
        let (Some(session), Some(thread)) = (self.session.take(), &self.thread) else {
            return;
        };
        stop_in_background(session, self.backend.store.clone(), thread.id);
    }
}
