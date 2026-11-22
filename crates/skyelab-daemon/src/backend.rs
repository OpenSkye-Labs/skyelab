//! What the daemon serves: [`Backend`] answers each RPC, [`StoreBackend`] is the real one.

use std::sync::Mutex;

use skyelab_protocol::{ErrorCode, Project, RpcError, Thread, ThreadEvent};
use skyelab_store::Store;

/// Answers RPC calls. Called from a blocking thread, so implementations may block.
pub trait Backend: Send + Sync + 'static {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError>;
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError>;
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError>;
    /// Returns the new thread's id.
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError>;
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError>;
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError>;
}

/// Runs agents for remote requests. The daemon never launches anything itself; an implementation must launch
/// with the folder's `AccountsConfig::launch_env`.
pub trait Agents: Send + Sync + 'static {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError>;
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError>;
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError>;
}

/// Every call fails with [`ErrorCode::NotWired`].
pub struct NotWired;

impl NotWired {
    fn err<T>() -> Result<T, RpcError> {
        Err(RpcError::new(
            ErrorCode::NotWired,
            "running agents remotely is not wired up yet",
        ))
    }
}

impl Agents for NotWired {
    fn start_thread(&self, _: i64, _: &str) -> Result<i64, RpcError> {
        Self::err()
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), RpcError> {
        Self::err()
    }
    fn interrupt(&self, _: i64) -> Result<(), RpcError> {
        Self::err()
    }
}

/// Keeps one reply well under a client's message limit; a client pages by asking again from the last seq.
pub const MAX_EVENTS_PER_REPLY: usize = 500;

/// Reads come from the store; agent calls go to `A`.
pub struct StoreBackend<A> {
    store: Mutex<Store>,
    agents: A,
}

impl<A: Agents> StoreBackend<A> {
    pub fn new(store: Store, agents: A) -> Self {
        Self {
            store: Mutex::new(store),
            agents,
        }
    }

    fn with_store<T>(&self, f: impl FnOnce(&Store) -> anyhow::Result<T>) -> Result<T, RpcError> {
        let store = self
            .store
            .lock()
            .map_err(|_| internal("store lock poisoned"))?;
        // The detail (paths, SQL) stays in this machine's log; the client gets a generic error.
        f(&store).map_err(|e| {
            eprintln!("skyelab-daemon: store error: {e:#}");
            internal("store error")
        })
    }
}

fn internal(message: &str) -> RpcError {
    RpcError::new(ErrorCode::Internal, message)
}

impl<A: Agents> Backend for StoreBackend<A> {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        let projects = self.with_store(|s| s.list_projects())?;
        Ok(projects
            .into_iter()
            .map(|p| Project {
                id: p.id,
                name: p.name,
                // The store only accepts UTF-8 paths, so this is lossless.
                path: p.path.to_string_lossy().into_owned(),
            })
            .collect())
    }

    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
        let threads = self.with_store(|s| s.list_threads(project_id))?;
        Ok(threads
            .into_iter()
            .map(|t| Thread {
                id: t.id,
                project_id: t.project_id,
                title: t.title,
                account_id: t.account_id,
                status: t.status,
                created_at: t.created_at,
                updated_at: t.updated_at,
            })
            .collect())
    }

    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        let events = self.with_store(|s| s.events_since(thread_id, seq))?;
        Ok(events
            .into_iter()
            .take(MAX_EVENTS_PER_REPLY)
            .map(event)
            .collect())
    }

    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError> {
        self.agents.start_thread(project_id, prompt)
    }

    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError> {
        self.agents.send_message(thread_id, text)
    }

    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError> {
        self.agents.interrupt(thread_id)
    }
}

/// Store event → wire event, for [`crate::Daemon::push_event`].
pub fn event(e: skyelab_store::Event) -> ThreadEvent {
    ThreadEvent {
        thread_id: e.thread_id,
        seq: e.seq,
        kind: e.kind,
        payload_json: e.payload_json,
        created_at: e.created_at,
    }
}
