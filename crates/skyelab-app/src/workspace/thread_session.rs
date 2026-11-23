//! A thread's agent session once started: the event pump that stores and shows each event,
//! and stopping, interrupting and reaping the agent.

use super::saves::save;
use super::{Interrupt, Record, SharedStore, Stop, ThreadView, ThreadViewEvent, lock};
use anyhow::{Context as _, Result};
use futures::future::Either;
use gpui::{AppContext as _, Context, Window};
use skyelab_agent::{Event, Session};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often a running session checks whether its agent exited.
const WATCH_EXIT: Duration = Duration::from_secs(1);
/// After the exit, how long the pump keeps storing what is still coming before the session counts as ended.
const DRAIN: Duration = Duration::from_secs(2);

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
        let session = Arc::new(Mutex::new(session));
        // Asks an earlier agent left open (it was killed, or SkyeLab quit) can't be answered by this one.
        let mut stale = Vec::new();
        for (ix, item) in self.items.iter_mut().enumerate() {
            if let crate::workspace::thread_model::Item::Approval(a) = item
                && a.decision.is_none()
            {
                a.decision = Some(skyelab_agent::Decision::Withdrawn);
                stale.push(ix);
            }
        }
        for ix in stale {
            self.list.splice(ix..ix + 1, 1);
        }
        let watched = Arc::downgrade(&session);
        self.session = Some(session);
        // The launch sent one prompt.
        let turns = Arc::new(AtomicUsize::new(1));
        self.turns = turns.clone();
        self.unsaved.set_session(&turns);
        let launch = self.launches;
        // A stopped session's pump keeps storing what it already printed.
        if let Some(old) = self._pump.take() {
            old.detach();
        }
        let unsaved = self.unsaved.clone();
        self._pump = Some(cx.spawn(async move |this, cx| {
            let mut exited = false;
            // stdout can outlive the agent (a tool it started holds it), so its exit is watched too, on a timer that
            // events don't restart.
            let mut tick = cx.background_executor().timer(WATCH_EXIT);
            // stderr to store next, ahead of the agent's next event.
            let mut output: Option<Event> = None;
            loop {
                let next = match output.take() {
                    Some(e) => Either::Left((Ok(e), ())),
                    None => match futures::future::select(events.recv_async(), &mut tick).await {
                        Either::Left((e, _)) => Either::Left((e, ())),
                        Either::Right(r) => Either::Right(r),
                    },
                };
                let event = match next {
                    Either::Left((Ok(event), _)) => event,
                    Either::Left((Err(_), _)) => break, // stdout closed
                    Either::Right(_) if exited => break, // DRAIN passed since the exit
                    Either::Right(_) => {
                        let s = watched.clone();
                        exited = cx
                            .background_spawn(async move {
                                s.upgrade().is_none_or(|s| {
                                    lock(&s)
                                        .and_then(|mut s| s.try_wait())
                                        .is_ok_and(|st| st.is_some())
                                })
                            })
                            .await;
                        tick =
                            cx.background_executor()
                                .timer(if exited { DRAIN } else { WATCH_EXIT });
                        continue;
                    }
                };
                // Streaming text is shown, never stored: the complete block that follows is.
                // After Stop, what streamed is already kept as a partial message (`finish`), so a killed agent's
                // last deltas aren't shown as a second one.
                if matches!(event, Event::Delta { .. }) {
                    let record = Record::Agent(event);
                    let shown = this.update(cx, |this, cx| {
                        if !this.stop_requested && launch == this.launches {
                            this.push(&record, cx)
                        }
                    });
                    if shown.is_err() {
                        return;
                    }
                    continue;
                }
                // The turn ends: text it cut off is stored ahead of its result.
                if matches!(event, Event::Result { .. })
                    && this.update(cx, |this, cx| this.keep_partial(cx)).is_err()
                {
                    return;
                }
                let gone = skyelab_agent::session_gone(&event);
                // An interrupt ends a Codex turn as an error too; the user asked for that one.
                let failed = matches!(&event, Event::Result { is_error: true, result, .. }
                    if result.as_deref() != Some("interrupted"));
                let record = Record::Agent(event);
                let left = save(&unsaved, &store, id, record.clone(), &turns, cx).await;
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
                    if gone {
                        this.rerun = this.resumed_prompt.clone();
                    } else if matches!(record, Record::Agent(Event::Init { .. })) {
                        this.resumed_prompt = None;
                    }
                    this.push(&record, cx);
                    if matches!(record, Record::Agent(Event::Result { .. })) {
                        this.checkpoint_turn_end(cx);
                        cx.emit(ThreadViewEvent::TurnDone);
                        this.after_turn(cx);
                        this.title_after_turn(cx);
                    }
                });
                if shown.is_err() {
                    return;
                }
                if failed {
                    output = stderr_of(&watched, false, cx).await;
                }
            }
            // A nonzero exit says why on stderr; one the user asked for (Stop) doesn't need explaining. Text the end cut
            // off is stored first, so a reload shows it above that output.
            let stopped = this
                .update(cx, |this, cx| {
                    let stopped = this.stop_requested || launch != this.launches;
                    if launch == this.launches {
                        this.keep_partial(cx);
                    }
                    stopped
                })
                .unwrap_or(true);
            if !stopped && let Some(event) = stderr_of(&watched, true, cx).await {
                let record = Record::Agent(event);
                let left = save(&unsaved, &store, id, record.clone(), &turns, cx).await;
                let _ = this.update(cx, |this, cx| {
                    this.saved(left, cx);
                    if matches!(&record, Record::Agent(e) if skyelab_agent::session_gone(e)) {
                        this.rerun = this.resumed_prompt.clone();
                    }
                    this.push(&record, cx);
                });
            }
            let _ = this.update(cx, |this, cx| this.ended(launch, cx));
        }));
    }

    /// A turn ended: its checkpoint is queued behind earlier sends, and before the next turn's starting one. Changes
    /// tabs refresh once it is taken.
    fn checkpoint_turn_end(&mut self, cx: &mut Context<Self>) {
        let Some(t) = &self.thread else { return };
        let (cwd, key) = (t.cwd.clone(), crate::workspace::thread_checkpoints::key(t));
        let prev = self.send_chain.take();
        let work = cx.background_executor().clone();
        self.send_chain = Some(cx.spawn(async move |this, cx| {
            if let Some(prev) = prev {
                prev.await;
            }
            work.spawn(async move { crate::workspace::thread_checkpoints::after_turn(&cwd, &key) })
                .await;
            let _ = this.update(cx, |_, cx| cx.emit(ThreadViewEvent::FilesChanged));
        }));
    }

    /// claude's stdout closed and every event is stored: record how the thread ended.
    fn ended(&mut self, launch: u64, cx: &mut Context<Self>) {
        if launch != self.launches {
            return; // a stopped session's stream; Stop already recorded it
        }
        // Gone mid-turn: that turn's end is still a checkpoint, so it can be reviewed and rewound.
        if self.turns.swap(0, Ordering::SeqCst) > 0 {
            self.checkpoint_turn_end(cx);
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
        self.turns.store(0, Ordering::SeqCst); // no agent, so no turn is running
        self.finish(cx);
        if let Some(text) = self.rerun.take().filter(|_| !self.stop_requested) {
            self.resumed_prompt = None;
            match self.start_fresh(text, cx) {
                Ok(()) => {
                    self.in_background(cx, move || {
                        drop(session);
                        Ok(())
                    });
                    return;
                }
                Err(why) => self.error = Some(why.into()),
            }
        }
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
        self.keep_partial(cx);
        if let Some(first) = crate::workspace::thread_model::settle_live(&mut self.items) {
            let n = self.items.len();
            self.list.splice(first..n, n - first);
        }
        // A fork has no session until its first send starts one (with its context prompt).
        self.read_only = self
            .thread
            .as_ref()
            .is_none_or(|t| t.agent_session_id.is_none() && t.forked_from.is_none());
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
            if self.turn_running() {
                self.interrupt_pending(cx);
            }
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
                // Stopped mid-turn: its end checkpoint is queued now, before a resume can start the next turn.
                if this.turns.swap(0, Ordering::SeqCst) > 0 {
                    this.checkpoint_turn_end(cx);
                }
                this.finish(cx);
            });
        }));
    }

    /// Runs blocking work on the background executor and shows its error, if any, inline.
    pub(in crate::workspace) fn in_background(
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
    pub(super) fn quitting(&mut self) -> impl std::future::Future<Output = ()> + use<> {
        self.stop_requested = true; // a late `ended` must not write exited over stopped
        let job = self.session.take().zip(self.thread.as_ref().map(|t| t.id));
        let store = self.backend.store.clone();
        let drafts = self.draft_flush();
        async move {
            drafts();
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

/// How long an agent whose stdout closed gets to exit before its exit status is read.
const EXIT_WAIT: Duration = Duration::from_secs(1);

/// The agent's stderr since it was last taken, as an event to store, or `None` when there is none. With
/// `exited`, only after a nonzero exit (waiting up to [`EXIT_WAIT`] for the exit).
async fn stderr_of(
    session: &std::sync::Weak<Mutex<Session>>,
    exited: bool,
    cx: &mut gpui::AsyncApp,
) -> Option<Event> {
    let session = session.upgrade()?;
    let timer = cx.background_executor().clone();
    let deadline = std::time::Instant::now() + EXIT_WAIT;
    let text = cx
        .background_spawn(async move {
            loop {
                if !exited {
                    break lock(&session).ok().map(|s| s.take_stderr());
                }
                let status = lock(&session).ok()?.try_wait().ok()?;
                match status {
                    Some(status) if status.success() => break None,
                    Some(_) => {
                        // The stderr thread may still be draining the pipe.
                        timer.timer(Duration::from_millis(100)).await;
                        break lock(&session).ok().map(|s| s.take_stderr());
                    }
                    None if std::time::Instant::now() < deadline => {
                        timer.timer(Duration::from_millis(50)).await
                    }
                    None => break None,
                }
            }
        })
        .await?;
    let text = text.trim_end();
    (!text.is_empty()).then(|| Event::Stderr {
        text: text.to_owned(),
    })
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

impl ThreadView {
    /// Ends the agent and makes a last try for held events, off the UI thread. `Workspace::close_thread` calls it, as
    /// the view may outlive the close by a frame; dropping the view runs it too, and a second run does nothing. A
    /// launch still in progress sees `stop_requested` and ends its own agent.
    pub(in crate::workspace) fn close(&mut self) {
        self.stop_requested = true;
        let session = self.session.take();
        let Some(thread) = &self.thread else {
            return;
        };
        if session.is_none() && self.unsaved_events == 0 {
            return;
        }
        self.unsaved_events = 0;
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

impl Drop for ThreadView {
    /// Closing the thread ends its agent. A launch still in progress ends its own (`NewSession`).
    fn drop(&mut self) {
        self.close();
    }
}
