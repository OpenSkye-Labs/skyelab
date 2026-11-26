//! What a sidebar thread row shows of its thread's session (status, an approval wait, the working timer) and which
//! account and binding its project folder resolves to.

use super::panels::Ask;
use super::{Workspace, thread::ThreadView};
use crate::ui::Status;
use gpui::{AppContext as _, Context, Entity, Window};
use skyelab_store::Thread;
use std::collections::HashMap;
use std::path::PathBuf;

impl Workspace {
    /// Each open view's session state, by thread id: an open view knows it now, the stored row only as of its last load.
    pub(super) fn live_rows(&self, cx: &gpui::App) -> HashMap<i64, RowState> {
        self.open_threads
            .iter()
            .filter_map(|v| {
                let v = v.read(cx);
                let state = RowState {
                    status: v.live_status(),
                    ask: waiting_ask(v),
                    started_ms: v.turn_started_ms,
                };
                Some((v.thread.as_ref()?.id, state))
            })
            .collect()
    }

    /// Resolves each project's account in the background (it touches the disk), then redraws the sidebar. A newer
    /// call replaces (cancels) an older one, so the latest config wins.
    pub(super) fn resolve_project_accounts(&mut self, cx: &mut Context<Self>) {
        let config = self.config.clone();
        let projects: Vec<(i64, PathBuf)> = self
            .projects
            .iter()
            .map(|p| (p.id, p.path.clone()))
            .collect();
        let task = cx.background_spawn(async move {
            let mut bindings = std::collections::HashMap::new();
            let accounts = projects
                .into_iter()
                .map(|(id, path)| {
                    if let Some(b) = config.binding_for(&path) {
                        bindings.insert(id, b.path.clone());
                    }
                    (id, config.resolve(&path).cloned())
                })
                .collect();
            (accounts, bindings)
        });
        self._project_accounts = Some(cx.spawn(async move |this, cx| {
            let (resolved, bindings) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.project_accounts = resolved;
                this.previews.bindings = bindings;
                this.sync_gh_sources(cx);
                this.refresh_sidebar(cx);
            });
        }));
    }
}

/// A thread row's session state: what its status says, what it waits on the user for, when its turn started.
#[derive(Clone, Copy)]
pub(super) struct RowState {
    pub status: Status,
    pub ask: Option<Ask>,
    pub started_ms: Option<i64>,
}

impl RowState {
    pub(super) fn running(self) -> bool {
        matches!(self.status, Status::Starting | Status::Running)
    }

    /// On the Working shelf: busy and not waiting on the user. A thread waiting for an answer is the user's turn, so
    /// it lists by time with its ask badge, as T3's `isThreadWorking` does.
    pub(super) fn working(self) -> bool {
        self.running() && self.ask.is_none()
    }
}

/// The state a row shows: its open view's, else its stored row's (a thread working with no view open).
pub(super) fn row_state(live: &HashMap<i64, RowState>, t: &Thread) -> RowState {
    live.get(&t.id).copied().unwrap_or(RowState {
        status: Status::parse(&t.status),
        ask: None,
        started_ms: None,
    })
}

/// What the thread's agent waits on the user for: a permission, a question or a plan (T3's sidebar status).
pub(super) fn waiting_ask(v: &ThreadView) -> Option<Ask> {
    // shortcut: only the last 32 items are checked (a waiting agent adds nothing after its ask, so it is near the
    // end); scan further if asks ever get buried.
    v.session.as_ref()?;
    v.items.iter().rev().take(32).find_map(|i| match i {
        super::thread_model::Item::Approval(a) if a.decision.is_none() => Some(Ask::of(&a.tool)),
        _ => None,
    })
}

/// What a sidebar row shows of an open view. The sidebar is rebuilt when this changes, not on every notify (a streamed
/// delta or a keystroke notifies the view too).
#[derive(Clone, PartialEq)]
pub(super) struct RowSig {
    id: Option<i64>,
    state: (Status, Option<Ask>, Option<i64>),
    pr_url: Option<String>,
}

impl RowSig {
    /// The agent waits on the user's answer.
    pub(super) fn asks(&self) -> bool {
        self.state.1.is_some()
    }
}

pub(super) fn row_sig(v: &ThreadView) -> RowSig {
    RowSig {
        id: v.thread.as_ref().map(|t| t.id),
        state: (v.live_status(), waiting_ask(v), v.turn_started_ms),
        pr_url: v.thread.as_ref().and_then(|t| t.pr_url.clone()),
    }
}

/// An open view's subscriptions, and the row state the sidebar last drew for it.
pub(super) struct ThreadSubs {
    pub sig: RowSig,
    pub _subs: [gpui::Subscription; 2],
}

impl Workspace {
    /// An open view notified: the sidebar is rebuilt when what its row shows changed, not on every notify; a
    /// background run that starts waiting on a permission answer says so.
    pub(super) fn thread_changed(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Cheap, and a parent's Lineage may have loaded after the child's last change.
        self.sync_lineage(view, cx);
        let sig = row_sig(view.read(cx));
        let asked = match self.thread_subs.get_mut(&view.entity_id()) {
            Some(subs) if subs.sig == sig => return,
            Some(subs) => {
                let asked = sig.asks() && !subs.sig.asks();
                subs.sig = sig;
                asked
            }
            None => false,
        };
        // A background run (a scheduled one has nobody watching) waits on this answer: say so.
        if asked && view.read(cx).owned_by(window) {
            self.approval_needed(view, window, cx);
        }
        self.refresh_sidebar(cx)
    }
}
