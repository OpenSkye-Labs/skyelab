//! Ending a thread view's agent when SkyeLab quits or the view closes, and when a start is abandoned.

use super::{SharedStore, ThreadView, lock};
use skyelab_agent::Session;

impl ThreadView {
    /// Writes where the reader left the thread, so it reopens there after a restart. Nothing before its rows
    /// loaded: an unloaded list follows its end and would wipe the kept mark.
    fn mark_flush(&self) -> impl FnOnce() + use<> {
        let job = self
            .thread
            .as_ref()
            .filter(|_| !self.items.is_empty())
            .map(|t| {
                (
                    t.id,
                    self.lists.mark(self.window),
                    self.backend.store.clone(),
                )
            });
        move || {
            if let Some((id, mark, store)) = job
                && let Ok(store) = lock(&store)
            {
                let _ = store.set_scroll_mark(id, mark);
            }
        }
    }

    pub(super) fn quitting(&mut self) -> impl std::future::Future<Output = ()> + use<> {
        self.stop_requested = true; // a late `ended` must not write exited over stopped
        let job = self.session.take().zip(self.thread.as_ref().map(|t| t.id));
        let store = self.backend.store.clone();
        let drafts = self.draft_flush();
        let mark = self.mark_flush();
        async move {
            drafts();
            mark();
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

impl ThreadView {
    /// Ends the agent and makes a last try for held events, off the UI thread. `Workspace::close_thread` calls it, as
    /// the view may outlive the close by a frame; dropping the view runs it too, and a second run does nothing. A
    /// launch still in progress sees `stop_requested` and ends its own agent.
    pub(in crate::workspace) fn close(&mut self) {
        self.stop_requested = true;
        let mark = self.mark_flush();
        let _ = std::thread::Builder::new()
            .name("skyelab-scroll-mark".into())
            .spawn(mark);
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
