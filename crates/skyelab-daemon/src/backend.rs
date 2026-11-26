//! What the daemon serves: [`Backend`] answers each RPC, [`StoreBackend`] is the real one.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use skyelab_core::AccountsConfig;

use skyelab_protocol::{
    ErrorCode, PREVIEW_CHARS, PermissionAnswer, Project, RpcError, ShellDelta, ShellSnapshot,
    Thread, ThreadEvent, ThreadRow,
};
use skyelab_store::{RowChange, Store};

use crate::shell::{Hub, ShellFeed};

/// Answers RPC calls. Called from a blocking thread, so implementations may block.
pub trait Backend: Send + Sync + 'static {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError>;
    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError>;
    fn thread_events_since(&self, thread_id: i64, seq: i64) -> Result<Vec<ThreadEvent>, RpcError>;
    /// Returns the new thread's id.
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError>;
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError>;
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError>;
    fn answer_permission(&self, _: i64, _: &str, _: PermissionAnswer) -> Result<(), RpcError> {
        NotWired::err()
    }
    fn answer_questions(&self, _: i64, _: &str, _: &[Vec<String>]) -> Result<(), RpcError> {
        NotWired::err()
    }
    /// Settled, snoozed until a time, or neither (never both: [`StoreBackend`] refuses that).
    fn shelve_thread(&self, _: i64, _: bool, _: Option<i64>) -> Result<(), RpcError> {
        NotWired::err()
    }
    /// Every listed project and thread, then each change to them (1.7). See [`ShellFeed`].
    fn subscribe_shell(&self) -> Result<ShellFeed, RpcError> {
        NotWired::err()
    }
}

/// Runs agents for remote requests. The daemon never launches anything itself. An implementation resolves the
/// folder's account once (`AccountsConfig::resolve`), records it on the thread row, and passes that same account
/// to `skyelab_agent::start`, which aborts if the folder resolves elsewhere by spawn time.
pub trait Agents: Send + Sync + 'static {
    fn start_thread(&self, project_id: i64, prompt: &str) -> Result<i64, RpcError>;
    fn send_message(&self, thread_id: i64, text: &str) -> Result<(), RpcError>;
    fn interrupt(&self, thread_id: i64) -> Result<(), RpcError>;
    /// Answers the permission request `request_id` the thread's agent waits on.
    fn answer_permission(&self, _: i64, _: &str, _: PermissionAnswer) -> Result<(), RpcError> {
        NotWired::err()
    }
    /// Answers the AskUserQuestion `request_id` the thread's agent waits on: one list of labels per question.
    fn answer_questions(&self, _: i64, _: &str, _: &[Vec<String>]) -> Result<(), RpcError> {
        NotWired::err()
    }
    /// Puts the thread on a shelf through the app, so its sidebar changes with its store.
    fn shelve_thread(&self, _: i64, _: bool, _: Option<i64>) -> Result<(), RpcError> {
        NotWired::err()
    }
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
    /// Started by the first `subscribe_shell`, so a backend nobody streams from runs no worker.
    hub: OnceLock<Hub>,
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
            hub: OnceLock::new(),
        }
    }

    fn accounts(&self) -> AccountsConfig {
        accounts(self.accounts.as_deref())
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
        Ok(projects.into_iter().map(wire_project).collect())
    }

    fn list_threads(&self, project_id: i64) -> Result<Vec<Thread>, RpcError> {
        let (threads, mut previews) = self.with_store(|s| {
            Ok((
                s.list_threads(project_id)?,
                s.last_replies(Some(project_id))?,
            ))
        })?;
        let accounts = self.accounts();
        Ok(threads
            .into_iter()
            .map(|t| {
                let preview = previews.remove(&t.id);
                wire_thread(t, preview, &accounts)
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

    fn answer_permission(
        &self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
    ) -> Result<(), RpcError> {
        self.agents
            .answer_permission(thread_id, request_id, decision)
    }

    fn answer_questions(
        &self,
        thread_id: i64,
        request_id: &str,
        answers: &[Vec<String>],
    ) -> Result<(), RpcError> {
        self.agents.answer_questions(thread_id, request_id, answers)
    }

    fn shelve_thread(
        &self,
        thread_id: i64,
        settled: bool,
        snoozed_until: Option<i64>,
    ) -> Result<(), RpcError> {
        if settled && snoozed_until.is_some() {
            let both = "a thread is settled or snoozed, not both";
            return Err(RpcError::new(ErrorCode::BadRequest, both));
        }
        self.agents.shelve_thread(thread_id, settled, snoozed_until)
    }

    fn subscribe_shell(&self) -> Result<ShellFeed, RpcError> {
        let hub = match self.hub.get() {
            Some(hub) => hub,
            None => {
                let path = self.accounts.clone();
                let read = Box::new(move |s: &Mutex<Store>, rows: &[RowChange]| {
                    // Read before the store's lock, which the app's UI thread may be waiting on.
                    let accounts = accounts(path.as_deref());
                    match s.lock() {
                        Ok(s) => read_rows(&s, rows, &accounts),
                        Err(_) => None,
                    }
                });
                let hub = Hub::start(&self.store, read).map_err(|e| {
                    eprintln!("skyelab-daemon: can't start the shell stream: {e}");
                    internal("can't start the shell stream")
                })?;
                // Two first subscribers at once: the loser's hub is dropped, its worker ends on the next write.
                let _ = self.hub.set(hub);
                self.hub.get().ok_or_else(|| internal("shell stream"))?
            }
        };
        hub.subscribe(|| {
            let (projects, threads, mut previews) = self.with_store(|s| {
                Ok((
                    s.list_projects()?,
                    s.list_all_threads()?,
                    s.last_replies(None)?,
                ))
            })?;
            let accounts = self.accounts();
            Ok(ShellSnapshot {
                seq: 0,
                projects: projects.into_iter().map(wire_project).collect(),
                threads: threads
                    .into_iter()
                    .map(|t| {
                        let preview = previews.remove(&t.id);
                        wire_thread(t, preview, &accounts)
                    })
                    .collect(),
                truncated: false,
            })
        })
    }
}

/// Read on each listing, so a label edited in Settings shows on the next one. A file that doesn't load gives
/// no labels, never stale ones.
fn accounts(path: Option<&Path>) -> AccountsConfig {
    let Some(path) = path else {
        return AccountsConfig::default();
    };
    AccountsConfig::load(path).unwrap_or_else(|e| {
        eprintln!("skyelab-daemon: no account labels: {e:#}");
        AccountsConfig::default()
    })
}

fn wire_project(p: skyelab_store::Project) -> Project {
    Project {
        id: p.id,
        name: p.name,
        // The store only accepts UTF-8 paths, so this is lossless.
        path: p.path.to_string_lossy().into_owned(),
        created_at: p.created_at,
    }
}

/// A store row as clients see it. `preview` is already cut to `PREVIEW_CHARS` by the store.
fn wire_thread(
    t: skyelab_store::Thread,
    preview: Option<String>,
    accounts: &AccountsConfig,
) -> Thread {
    let path = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().into_owned());
    Thread {
        id: t.id,
        project_id: t.project_id,
        title: t.title,
        account_label: accounts
            .accounts
            .iter()
            .find(|a| a.id == t.account_id)
            .map(|a| a.label.clone()),
        account_id: t.account_id,
        status: t.status,
        pinned: t.pinned_at.is_some(),
        preview,
        settled_at: t.settled_at,
        snoozed_until: t.snoozed_until,
        created_at: t.created_at,
        updated_at: t.updated_at,
        row: ThreadRow {
            cwd: t.cwd.to_string_lossy().into_owned(),
            permission_mode: t.permission_mode,
            agent_session_id: t.agent_session_id,
            worktree_path: path(t.worktree_path),
            worktree_branch: t.worktree_branch,
            worktree_base: t.worktree_base,
            pr_url: t.pr_url,
            pr_state: t.pr_state,
            parent_thread_id: t.parent_thread_id,
            pinned_at: t.pinned_at,
            archived_at: t.archived_at,
            unread: t.unread,
            forked_from: t.forked_from,
            side_of: t.side_of,
            compare_group: t.compare_group,
            scheduled_task_id: t.scheduled_task_id,
            limit_resume_at: t.limit_resume_at,
        },
    }
}

/// Changed rows as they read now: listed, or removed. A project listed again brings its threads back.
/// `None` when one can't be read: its change would be lost, so the caller restarts every stream.
fn read_rows(
    store: &Store,
    rows: &[RowChange],
    accounts: &AccountsConfig,
) -> Option<Vec<ShellDelta>> {
    // A preview that can't be read fails the batch too: an update without it would blank the row's last reply.
    let thread = |t: skyelab_store::Thread| {
        let preview = store.last_reply(t.id, PREVIEW_CHARS)?;
        anyhow::Ok(ShellDelta::ThreadUpdated(Box::new(wire_thread(
            t, preview, accounts,
        ))))
    };
    let mut out = Vec::new();
    for row in rows {
        let read = match *row {
            RowChange::Project(id) => store.shell_project(id).and_then(|p| {
                let Some(p) = p else {
                    return Ok(vec![ShellDelta::ProjectRemoved { id }]);
                };
                let threads = store.list_threads(id)?;
                let mut deltas = vec![ShellDelta::ProjectUpdated(wire_project(p))];
                for t in threads {
                    deltas.push(thread(t)?);
                }
                Ok(deltas)
            }),
            RowChange::Thread(id) => store.shell_thread(id).and_then(|t| match t {
                Some(t) => Ok(vec![thread(t)?]),
                None => Ok(vec![ShellDelta::ThreadRemoved { id }]),
            }),
        };
        match read {
            Ok(deltas) => out.extend(deltas),
            Err(e) => {
                eprintln!("skyelab-daemon: shell read failed, restarting the streams: {e:#}");
                return None;
            }
        }
    }
    Some(out)
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

/// The snapshot as one message under [`MAX_REPLY_BYTES`], for the wire: the newest threads that fit, archived ones
/// only after every active one, marked `truncated` when some were left out. This machine's own (in-process) snapshot
/// is never cut.
// shortcut: projects are never cut; a machine with megabytes of project rows alone would still overflow.
pub(crate) fn bound(mut s: ShellSnapshot) -> ShellSnapshot {
    let len = |v: serde_json::Result<Vec<u8>>| v.map_or(usize::MAX, |v| v.len());
    // The envelope (`{"type":"response","id":…,"result":{"kind":"shell","data":{"seq":…}}}`) fits in this.
    let mut bytes = 256 + len(serde_json::to_vec(&s.projects));
    if bytes.saturating_add(len(serde_json::to_vec(&s.threads))) <= MAX_REPLY_BYTES {
        return s;
    }
    let before = s.threads.len();
    // Stable: newest first within active, then archived.
    s.threads.sort_by_key(|t| t.row.archived_at.is_some());
    let mut full = false;
    s.threads.retain(|t| {
        if !full {
            bytes = bytes.saturating_add(len(serde_json::to_vec(t)) + 1);
            full = bytes > MAX_REPLY_BYTES;
        }
        !full
    });
    s.threads
        .sort_by_key(|t| (std::cmp::Reverse(t.updated_at), std::cmp::Reverse(t.id)));
    s.truncated = s.threads.len() < before;
    s
}

/// Bytes `ev` takes inside a reply.
fn encoded_len(ev: &ThreadEvent) -> usize {
    serde_json::to_vec(ev).map_or(usize::MAX, |v| v.len())
}
