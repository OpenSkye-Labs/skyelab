//! A thread's agent session once started: the event pump that stores and shows each event,
//! and stopping, interrupting and reaping the agent.

use super::saves::save;
use super::{Interrupt, Record, SharedStore, Stop, ThreadView, ThreadViewEvent, lock};
use anyhow::{Context as _, Result};
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::{Event, Session};
use std::sync::{Arc, Mutex};

impl ThreadView {
    /// Each event is stored before it is shown, so the store keeps stdout's order. An event the store refuses is
    /// still shown, and held with every later one in the view's queue until a retry saves them (`thread_saves`).
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
        let unsaved = self.unsaved.clone();
        self._pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv_async().await {
                let record = Record::Agent(event);
                let left = save(&unsaved, &store, id, record.clone(), cx).await;
                let shown = this.update(cx, |this, cx| {
                    this.saved(left, cx);
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

impl Drop for ThreadView {
    /// Closing the tab ends its agent. Killing and reaping waits, so it happens off the UI thread.
    fn drop(&mut self) {
        let session = self.session.take();
        let Some(thread) = &self.thread else {
            return;
        };
        if session.is_none() && self.unsaved_events == 0 {
            return;
        }
        let (store, id, unsaved) = (self.backend.store.clone(), thread.id, self.unsaved.clone());
        let _ = std::thread::Builder::new()
            .name("skyelab-agent-reaper".into())
            .spawn(move || {
                let stopped = session.is_some();
                drop(session);
                // A last try for held events; past this they are lost, as the warning said.
                let _ = unsaved.flush(&store, id);
                if stopped && let Ok(store) = lock(&store) {
                    let _ = store.set_thread_status(id, "stopped");
                }
            });
    }
}
