//! This machine's shell stream: a thread another writer adds to the store (a remote call, an agent, another
//! window) or deletes shows here without a reload. Unlike a remote machine's rows, a row this window holds is never
//! replaced from the stream: the window edits its rows before their store writes land, so a streamed row read in
//! between is older than the one shown (it would undo a mark-read or a new title until the next change).

use super::Workspace;
use gpui::{AppContext as _, Context};
use skyelab_env::Source as _;
use skyelab_protocol::ShellDelta;

impl Workspace {
    /// Follows this machine's shell. A stream that ends (it fell behind, or a row couldn't be read) reloads the
    /// list and follows again.
    pub(super) fn follow_local_shell(&mut self, cx: &mut Context<Self>) {
        let local = self.local.clone();
        let subscribe = cx.background_spawn(async move { local.subscribe_shell() });
        self._local_shell = Some(cx.spawn(async move |this, cx| {
            // A store that can't be read: the list's own load shows why.
            let Ok(feed) = subscribe.await else { return };
            while let Ok(change) = feed.changes.recv_async().await {
                if this
                    .update(cx, |this, cx| this.apply_local(change.delta, cx))
                    .is_err()
                {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.follow_local_shell(cx);
                this.load_threads(cx);
            });
        }));
    }

    fn apply_local(&mut self, delta: ShellDelta, cx: &mut Context<Self>) {
        match delta {
            ShellDelta::ThreadUpdated(t) => {
                if let Some(preview) = &t.preview {
                    self.previews.replies.insert(t.id, preview.clone());
                }
                if self.threads.iter().any(|r| r.id == t.id) {
                    return self.refresh_sidebar(cx);
                }
                self.threads.push(skyelab_env::store_thread(&t));
                self.threads
                    .sort_by_key(|t| (std::cmp::Reverse(t.updated_at), std::cmp::Reverse(t.id)));
            }
            ShellDelta::ThreadRemoved { id } => self.threads.retain(|t| t.id != id),
            // Rare (add, rename, remove): a reload also reselects and re-resolves accounts.
            ShellDelta::ProjectUpdated(_) | ShellDelta::ProjectRemoved { .. } => {
                return self.load_projects(None, cx);
            }
        }
        self.refresh_sidebar(cx);
        cx.notify();
    }
}
