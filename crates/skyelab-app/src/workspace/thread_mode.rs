//! The thread's permission mode: picking it, the Full access confirm, and writing it to the agent and the store.

use super::ThreadView;
use crate::workspace::lock;
use gpui::Context;
use skyelab_agent::Mode;

impl ThreadView {
    /// Picks the thread's permission mode. Full access waits for [`Self::confirm_mode`].
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.is_side_chat() && mode != Mode::Plan {
            return cx.notify();
        }
        if mode == Mode::Bypass && self.mode != Mode::Bypass {
            self.confirm_bypass = true;
            return cx.notify();
        }
        self.apply_mode(mode, cx);
    }

    /// The user confirmed Full access (or turned it down).
    pub fn confirm_mode(&mut self, yes: bool, cx: &mut Context<Self>) {
        let asked = std::mem::take(&mut self.confirm_bypass);
        if yes && asked {
            self.apply_mode(Mode::Bypass, cx);
        }
        cx.notify();
    }

    fn apply_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.confirm_bypass = false;
        if self.mode == mode {
            return cx.notify();
        }
        self.mode = mode;
        self.write_mode(cx);
        cx.notify();
    }

    /// Sends the newest mode to the running agent, then to the store row.
    pub(in crate::workspace) fn write_mode(&mut self, cx: &mut Context<Self>) {
        if self.is_side_chat() {
            self.mode = Mode::Plan; // its agent never leaves Plan
        }
        let ix = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        self.mode_now.store(ix, std::sync::atomic::Ordering::SeqCst);
        let row = self.thread.as_mut().map(|t| {
            t.permission_mode = self.mode.as_str().to_owned();
            t.id
        });
        let (store, session) = (self.backend.store.clone(), self.session.clone());
        let (writes, now) = (self.mode_writes.clone(), self.mode_now.clone());
        self.in_background(cx, move || {
            let _one_at_a_time = lock(&writes)?;
            let mode = Mode::ALL[now.load(std::sync::atomic::Ordering::SeqCst)];
            // Each step runs even if the other fails, so a store error never leaves the agent on the old mode.
            let live = session.map_or(Ok(()), |s| lock(&s)?.set_mode(mode));
            let stored = row.map_or(Ok(()), |id| {
                lock(&store)?.set_thread_permission_mode(id, mode.as_str())
            });
            live.and(stored)
        });
    }
}
