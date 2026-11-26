//! The thread's Changes tab in the right dock: opening it, and opening it at a file.

use super::Workspace;
use super::changes::ChangesView;
use super::thread::ThreadView;
use gpui::{AppContext as _, Context, Entity, Window};

impl Workspace {
    /// The Changes tab for the thread's folder, in the right dock: refreshed and selected if one is open, else added.
    pub(super) fn open_changes(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ChangesView> {
        let dir = view.read(cx).cwd.clone();
        let base = view
            .read(cx)
            .thread
            .as_ref()
            .and_then(|t| t.worktree_base.clone());
        self.changes_views.retain(|c| c.upgrade().is_some());
        let open = self
            .changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .find(|c| c.read(cx).dir == dir && c.read(cx).base == base);
        if let Some(changes) = open {
            follow(&changes, view, cx);
            changes.update(cx, |c, cx| c.refresh(cx));
            self.show_right_panel(changes.entity_id().into(), window, cx);
            return changes;
        }
        let title = view.read(cx).title();
        let changes = cx.new(|cx| ChangesView::new(dir, base, title, Some(view), window, cx));
        self.add_right_panel(changes.clone(), window, cx);
        self.changes_views.push(changes.downgrade());
        cx.notify();
        changes
    }

    /// The center shows `view`: its folder's Changes tab follows it (title, publishing), as T3's diff panel follows
    /// the active thread, rather than naming the thread shown before.
    pub(super) fn follow_shown_thread(
        &mut self,
        view: &Entity<ThreadView>,
        cx: &mut Context<Self>,
    ) {
        let (dir, base) = {
            let v = view.read(cx);
            (
                v.cwd.clone(),
                v.thread.as_ref().and_then(|t| t.worktree_base.clone()),
            )
        };
        for changes in self.changes_views.iter().filter_map(|c| c.upgrade()) {
            let c = changes.read(cx);
            if c.pr.is_none() && c.dir == dir && c.base == base {
                follow(&changes, view, cx);
            }
        }
    }

    /// A file the thread's timeline names: its folder's Changes tab, scrolled to the file once the diff lists it.
    pub(super) fn open_path(
        &mut self,
        view: &Entity<ThreadView>,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_changes(view, window, cx);
        let dir = view.read(cx).cwd.clone();
        let base = view
            .read(cx)
            .thread
            .as_ref()
            .and_then(|t| t.worktree_base.clone());
        let tab = self
            .changes_views
            .iter()
            .filter_map(|c| c.upgrade())
            .find(|c| c.read(cx).dir == dir && c.read(cx).base == base);
        if let Some(changes) = tab {
            let path = path.to_owned();
            changes.update(cx, |c, cx| {
                // A refresh is on its way (open_changes started it): reveal against what it loads.
                c.reveal_after_load = Some(path.clone());
                c.reveal(&path, cx);
            });
        }
    }

    /// A thread got its generated title: its Changes tabs say it too.
    pub(super) fn retitle_changes(&mut self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let (thread, title) = (view.downgrade(), view.read(cx).title());
        for changes in self.changes_views.iter().filter_map(|c| c.upgrade()) {
            if changes.read(cx).thread.as_ref() == Some(&thread) {
                changes.update(cx, |c, cx| {
                    c.title = title.clone();
                    cx.notify();
                });
            }
        }
        cx.notify();
    }

    /// A thread's turn ended: every Changes tab on its folder may be out of date.
    pub(super) fn refresh_changes(&mut self, view: &Entity<ThreadView>, cx: &mut Context<Self>) {
        let dir = view.read(cx).cwd.clone();
        for changes in self.changes_views.iter().filter_map(|c| c.upgrade()) {
            if changes.read(cx).dir == dir {
                changes.update(cx, |c, cx| c.refresh(cx));
            }
        }
    }
}

/// One tab per folder; it publishes for (and saves the PR link on) the thread it follows. Same folder, same bound
/// account, and publishing re-checks the account anyway.
fn follow(changes: &Entity<ChangesView>, view: &Entity<ThreadView>, cx: &mut gpui::App) {
    let thread = view.downgrade();
    let title = view.read(cx).title();
    changes.update(cx, |c, cx| {
        // A comment being written for another thread doesn't move to this one.
        if c.thread.as_ref() != Some(&thread) {
            (c.draft, c.drag) = (None, None);
        }
        c.title = title;
        c.thread = Some(thread);
        cx.notify();
    });
}
