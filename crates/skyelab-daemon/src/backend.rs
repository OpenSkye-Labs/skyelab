//! What the daemon serves: [`Backend`] answers each RPC, [`StoreBackend`] is the real one.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use skyelab_core::AccountsConfig;

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

/// Runs agents for remote requests. The daemon never launches anything itself. An implementation resolves the
/// folder's account once (`AccountsConfig::resolve`), records it on the thread row, and passes that same account
/// to `skyelab_agent::start`, which aborts if the folder resolves elsewhere by spawn time.
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
/// Encoded events in one reply (or one push) stay under this: half the client's 16 MiB message limit.
pub const MAX_REPLY_BYTES: usize = 8 << 20;

/// Reads come from the store; agent calls go to `A`.
pub struct StoreBackend<A> {
    store: Arc<Mutex<Store>>,
    agents: A,
    accounts: Option<PathBuf>,
}

impl<A: Agents> StoreBackend<A> {
    /// A backend that sends no account labels.
    pub fn new(store: Store, agents: A) -> Self {
        Self::shared(Arc::new(Mutex::new(store)), agents, None)
    }

    /// Reads a store the app also writes, so both see the same threads (including an in-memory fallback).
    /// `accounts` is this machine's accounts.toml, which names each thread's account for clients.
    pub fn shared(store: Arc<Mutex<Store>>, agents: A, accounts: Option<PathBuf>) -> Self {
        Self {
            store,
            agents,
            accounts,
        }
    }

    /// Read on each listing, so a label edited in Settings shows on the next one. A file that doesn't load gives
    /// no labels, never stale ones.
    fn accounts(&self) -> AccountsConfig {
        let Some(path) = &self.accounts else {
            return AccountsConfig::default();
        };
        AccountsConfig::load(path).unwrap_or_else(|e| {
            eprintln!("skyelab-daemon: no account labels: {e:#}");
            AccountsConfig::default()
        })
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
        let accounts = self.accounts();
        let label = |id: &str| {
            accounts
                .accounts
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.label.clone())
        };
        Ok(threads
            .into_iter()
            .map(|t| Thread {
                id: t.id,
                project_id: t.project_id,
                title: t.title,
                account_label: label(&t.account_id),
                account_id: t.account_id,
                status: t.status,
                created_at: t.created_at,
                updated_at: t.updated_at,
            })
            .collect())
    }

    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        let events = self.with_store(|s| s.events_since(thread_id, seq))?;
        let mut bytes = 0;
        // At least one event per page, so paging always makes progress; `event` keeps each one under the cap.
        Ok(events
            .into_iter()
            .take(MAX_EVENTS_PER_REPLY)
            .map(event)
            .take_while(|e| {
                bytes += encoded_len(e);
                bytes <= MAX_REPLY_BYTES
            })
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

/// Store event → wire event, for [`crate::Daemon::push_event`]. One too big for [`MAX_REPLY_BYTES`] keeps its
/// seq and kind but carries a short notice instead, so a client never gets a frame over its limit.
pub fn event(e: skyelab_store::Event) -> ThreadEvent {
    let mut ev = ThreadEvent {
        thread_id: e.thread_id,
        seq: e.seq,
        kind: e.kind,
        payload_json: e.payload_json,
        created_at: e.created_at,
    };
    let len = encoded_len(&ev);
    if len > MAX_REPLY_BYTES {
        let notice = format!(
            "[This event is too large to show remotely ({} MiB). Open the thread on its own machine.]",
            len >> 20
        );
        // Same shape as the original: a prompt row is a JSON string, an agent row an `Event` (shown as text).
        ev.payload_json = if ev.payload_json.starts_with('"') {
            serde_json::Value::String(notice).to_string()
        } else {
            serde_json::json!({"Assistant": {"content": [{"type": "text", "text": notice}]}})
                .to_string()
        };
    }
    ev
}

/// Bytes `ev` takes inside a reply.
fn encoded_len(ev: &ThreadEvent) -> usize {
    serde_json::to_vec(ev).map_or(usize::MAX, |v| v.len())
}
