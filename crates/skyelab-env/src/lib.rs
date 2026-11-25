//! Every machine, this one included, is an environment the UI reads and drives through one [`Source`]. See
//! docs/architecture.md. No GPUI here.

mod local;
mod remote;

pub use local::LocalSource;
pub use remote::RemoteSource;
pub use skyelab_client::Error;

use skyelab_protocol::{PermissionAnswer, Project, Thread, ThreadEvent};

/// Which machine a row belongs to: the serving node's Tailscale StableID (`Hello.node_id`), never a host name.
/// Ids of rows (projects, threads) are only unique within one environment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EnvironmentId(String);

impl EnvironmentId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// This machine. shortcut: a fixed id until the registry (step 3) gives this machine its own node id.
    pub fn this_machine() -> Self {
        Self::new("local")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One environment's sidebar: its projects and their threads, as the protocol carries them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shell {
    pub projects: Vec<Project>,
    /// Project by project, in `projects` order; each project's newest first.
    pub threads: Vec<Thread>,
}

/// One environment's server API, a method per protocol call (`skyelab_protocol::Call`). Machine-bound features
/// (terminals, browser, devices, file preview) are not here: the UI offers them on this machine only.
///
/// Calls block: run them off the UI thread. A remote call fails with [`Error::Timeout`] after the client's
/// request timeout, so none blocks forever.
pub trait Source: Send + Sync {
    /// `None` until known (a remote one before its first hello).
    fn environment(&self) -> Option<EnvironmentId>;
    fn list_projects(&self) -> Result<Vec<Project>, Error>;
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, Error>;
    /// One page of events after `seq`, oldest first.
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, Error>;
    /// Returns the new thread's id. The serving machine resolves the folder's account; none is sent.
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, Error>;
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), Error>;
    fn interrupt(&self, thread_id: i64) -> Result<(), Error>;
    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
    ) -> Result<(), Error>;
    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), Error>;
    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), Error>;

    /// Every project and its threads. shortcut: one listing per project until protocol 1.7's shell snapshot.
    fn shell(&self) -> Result<Shell, Error> {
        let projects = self.list_projects()?;
        let mut threads = Vec::new();
        for p in &projects {
            threads.extend(self.list_threads(p.id)?);
        }
        Ok(Shell { projects, threads })
    }
}
