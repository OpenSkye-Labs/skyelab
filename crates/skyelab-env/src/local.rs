//! This machine: the daemon's own [`Backend`] called in-process (no socket, no TLS, no serde). It is the same
//! object the daemon dispatches remote calls to, so every check it makes applies here too.

use std::path::PathBuf;
use std::sync::Arc;

use skyelab_daemon::Backend;
use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};

use crate::{EnvironmentId, Error, ShellFeed, Source};

pub struct LocalSource {
    backend: Arc<dyn Backend>,
}

impl LocalSource {
    /// `backend` is the one this machine's daemon serves, so local reads pass the same checks remote ones do.
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self { backend }
    }
}

/// A shell project as this machine's store row, for UI code still typed on store rows.
/// shortcut: the sidebar keeps store rows until it draws every environment's shell (environments plan, step 4).
pub fn store_project(p: &Project) -> skyelab_store::Project {
    skyelab_store::Project {
        id: p.id,
        name: p.name.clone(),
        path: PathBuf::from(&p.path),
        created_at: p.created_at,
    }
}

/// A shell thread (1.7 row fields included) as this machine's store row. Lossless for this machine's own rows:
/// `tests/sidebar_rows.rs` checks it against the store.
pub fn store_thread(t: &Thread) -> skyelab_store::Thread {
    let r = &t.row;
    skyelab_store::Thread {
        id: t.id,
        project_id: t.project_id,
        title: t.title.clone(),
        cwd: PathBuf::from(&r.cwd),
        account_id: t.account_id.clone(),
        agent_session_id: r.agent_session_id.clone(),
        status: t.status.clone(),
        created_at: t.created_at,
        updated_at: t.updated_at,
        worktree_path: r.worktree_path.as_ref().map(PathBuf::from),
        worktree_branch: r.worktree_branch.clone(),
        worktree_base: r.worktree_base.clone(),
        pr_url: r.pr_url.clone(),
        parent_thread_id: r.parent_thread_id,
        permission_mode: r.permission_mode.clone(),
        pinned_at: r.pinned_at,
        archived_at: r.archived_at,
        unread: r.unread,
        forked_from: r.forked_from,
        pr_state: r.pr_state.clone(),
        side_of: r.side_of,
        compare_group: r.compare_group,
        scheduled_task_id: r.scheduled_task_id,
        settled_at: t.settled_at,
        snoozed_until: t.snoozed_until,
        limit_resume_at: r.limit_resume_at,
    }
}

impl Source for LocalSource {
    fn environment(&self) -> Option<EnvironmentId> {
        Some(EnvironmentId::this_machine())
    }
    fn list_projects(&self) -> Result<Vec<Project>, Error> {
        self.backend.list_projects().map_err(Error::Rpc)
    }
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, Error> {
        self.backend.list_threads(project_id).map_err(Error::Rpc)
    }
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, Error> {
        self.backend
            .thread_events_since(thread_id, seq)
            .map_err(Error::Rpc)
    }
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, Error> {
        self.backend
            .start_thread(project_id, prompt)
            .map_err(Error::Rpc)
    }
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), Error> {
        self.backend
            .send_message(thread_id, text)
            .map_err(Error::Rpc)
    }
    fn interrupt(&self, thread_id: i64) -> Result<(), Error> {
        self.backend.interrupt(thread_id).map_err(Error::Rpc)
    }
    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
    ) -> Result<(), Error> {
        self.backend
            .answer_permission(thread_id, request_id, decision)
            .map_err(Error::Rpc)
    }
    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), Error> {
        self.backend
            .answer_questions(thread_id, request_id, answers)
            .map_err(Error::Rpc)
    }
    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), Error> {
        self.backend
            .shelve_thread(thread_id, settled, snoozed_until)
            .map_err(Error::Rpc)
    }
    fn subscribe_shell(&self) -> Result<ShellFeed, Error> {
        let feed = self.backend.subscribe_shell().map_err(Error::Rpc)?;
        Ok(ShellFeed {
            snapshot: feed.snapshot,
            changes: feed.changes,
        })
    }
}
