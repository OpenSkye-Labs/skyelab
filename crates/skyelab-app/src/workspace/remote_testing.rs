//! What the remote-call tests read and drive on a Workspace.

use super::Workspace;
use gpui::Context;

impl Workspace {
    pub(crate) fn project_ids(&self) -> Vec<i64> {
        self.projects.iter().map(|p| p.id).collect()
    }

    pub(crate) fn reload_threads(&mut self, cx: &mut Context<Self>) {
        self.load_threads(cx);
    }

    /// Thread `id`'s row: (settled, snoozed until).
    pub(crate) fn row_shelf(&self, id: i64) -> Option<(bool, Option<i64>)> {
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.settled_at.is_some(), t.snoozed_until))
    }

    pub(crate) fn open_thread_ids(&self, cx: &gpui::App) -> Vec<i64> {
        self.open_threads
            .iter()
            .filter_map(|t| t.read(cx).thread.as_ref().map(|t| t.id))
            .collect()
    }
}
