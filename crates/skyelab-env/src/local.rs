//! This machine: the daemon's own [`Backend`] called in-process (no socket, no TLS, no serde). It is the same
//! object the daemon dispatches remote calls to, so every check it makes applies here too.

use std::sync::{Arc, Mutex};

use skyelab_daemon::Backend;
use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};
use skyelab_store::Store;

use crate::{EnvironmentId, Error, Source};

pub struct LocalSource {
    store: Arc<Mutex<Store>>,
    backend: Arc<dyn Backend>,
}

impl LocalSource {
    /// `backend` must read `store` (the app passes the one its daemon serves).
    pub fn new(store: Arc<Mutex<Store>>, backend: Arc<dyn Backend>) -> Self {
        Self { store, backend }
    }

    /// This machine's projects as full store rows, for its own sidebar.
    /// shortcut: the sidebar needs fields protocol rows lack (path as a folder, model, worktree, archive); move it
    /// onto [`Source::shell`] once protocol 1.7 grows `Thread` (environments plan, step 2).
    pub fn projects(&self) -> anyhow::Result<Vec<skyelab_store::Project>> {
        self.store()?.list_projects()
    }

    /// Every listed project's threads as full store rows, newest first. Same shortcut as [`Self::projects`].
    pub fn threads(&self) -> anyhow::Result<Vec<skyelab_store::Thread>> {
        self.store()?.list_all_threads()
    }

    fn store(&self) -> anyhow::Result<std::sync::MutexGuard<'_, Store>> {
        self.store
            .lock()
            .map_err(|_| anyhow::anyhow!("a background task panicked holding the lock"))
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
}
