//! The app's answers to SkyeLab's MCP tools. Reads run here, on the server's connection thread, against the
//! store and the current accounts. Starts and sends go to the Workspace on the UI thread, which checks the
//! account rule again with its own config and launches through the one `ThreadView` path.

use super::thread_model::Record;
use super::{SharedStore, lock};
use anyhow::Result;
use gpui::{App, Global};
use serde_json::{Value, json};
use skyelab_agent::mcp::{BrowserTool, Caller, Host, Proxy, Server, Tool};
use skyelab_agent::{Block, Event};
use skyelab_core::AccountsConfig;
use skyelab_store::Thread;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a start or send waits for the UI thread.
const UI_TIMEOUT: Duration = Duration::from_secs(30);
/// `read_thread` returns at most this much text (the end of it).
const MAX_READ: usize = 64 << 10;
/// A parent chain longer than this is broken data; it counts as too deep.
const MAX_CHAIN: u32 = 16;

pub type Reply<T> = flume::Sender<Result<T, String>>;

/// A delegated start, checked on the UI thread before it launches.
pub struct StartRequest {
    pub caller: Caller,
    /// The caller's delegation depth, from the store's parent chain.
    pub depth: u32,
    /// The caller's children still starting or running.
    pub live_children: usize,
    pub project_id: i64,
    pub prompt: String,
    pub worktree: bool,
}

/// What only the UI thread can do.
pub enum OrchCall {
    Start {
        request: StartRequest,
        reply: Reply<i64>,
    },
    Send {
        caller: Caller,
        thread_id: i64,
        text: String,
        reply: Reply<()>,
    },
    /// A browser tool, run against the caller's own tab (`browser_agent`).
    Browser {
        caller: Caller,
        tool: BrowserTool,
        reply: Reply<Value>,
    },
}

/// Present when orchestration is on (unix, and the socket could be bound).
pub struct Orchestration {
    pub server: Arc<Server>,
    pub host: Arc<AppHost>,
    pub proxy: Proxy,
}

impl Global for Orchestration {}

/// Starts the server in `run_dir` and makes it the app's. The receiver goes to `Workspace::set_orchestrator`.
pub fn install(
    cx: &mut App,
    run_dir: &Path,
    store: SharedStore,
    config: AccountsConfig,
    proxy: Proxy,
) -> Result<flume::Receiver<OrchCall>> {
    let (calls, rx) = flume::unbounded();
    let host = Arc::new(AppHost {
        store,
        config: Mutex::new(config),
        calls,
    });
    let server = Arc::new(Server::start(run_dir, host.clone())?);
    cx.set_global(Orchestration {
        server,
        host,
        proxy,
    });
    Ok(rx)
}

/// The accounts changed (Settings saved): tool calls check against these from now on.
pub fn set_config(cx: &App, config: &AccountsConfig) {
    if let Some(o) = cx.try_global::<Orchestration>()
        && let Ok(mut c) = o.host.config.lock()
    {
        *c = config.clone();
    }
}

pub struct AppHost {
    store: SharedStore,
    config: Mutex<AccountsConfig>,
    calls: flume::Sender<OrchCall>,
}

fn ask<T>(
    calls: &flume::Sender<OrchCall>,
    make: impl FnOnce(Reply<T>) -> OrchCall,
) -> Result<T, String> {
    let (reply, answer) = flume::bounded(1);
    calls
        .send(make(reply))
        .map_err(|_| "SkyeLab is closing".to_owned())?;
    // Dropping `answer` on a timeout marks the call abandoned, so the UI won't run it later.
    answer
        .recv_timeout(UI_TIMEOUT)
        .map_err(|_| "SkyeLab did not answer in time".to_owned())?
}

impl AppHost {
    fn config(&self) -> Result<AccountsConfig, String> {
        self.config
            .lock()
            .map(|c| c.clone())
            .map_err(|_| "accounts are unavailable".to_owned())
    }

    fn store<T>(&self, f: impl FnOnce(&skyelab_store::Store) -> Result<T>) -> Result<T, String> {
        let store = lock(&self.store).map_err(|e| e.to_string())?;
        f(&store).map_err(|e| format!("{e:#}"))
    }

    /// The thread `id` if this caller may read or drive it: one it delegated (directly or further down), still
    /// under the caller's account. Never the user's other threads, nor the caller's own parent.
    fn drivable(&self, caller: &Caller, id: i64) -> Result<Thread, String> {
        let not_found = || format!("no thread {id} you can reach");
        if !self.descends_from(id, caller.thread_id)? {
            return Err(format!(
                "thread {id} wasn't delegated by this thread; you can only reach threads you started"
            ));
        }
        let (thread, project) = self.store(|s| {
            let thread = s.thread(id)?;
            let project = match &thread {
                Some(t) => s.project(t.project_id)?,
                None => None,
            };
            Ok((thread, project))
        })?;
        let (Some(thread), Some(project)) = (thread, project) else {
            return Err(not_found());
        };
        self.config()?
            .check_drive(&caller.account_id, &thread.account_id, &project.path)
            .map_err(|why| format!("thread {id}: {why}"))?;
        Ok(thread)
    }

    /// Whether `ancestor` is on `id`'s parent chain (`id` itself doesn't count).
    fn descends_from(&self, id: i64, ancestor: i64) -> Result<bool, String> {
        let mut next = self
            .store(|s| s.thread(id))?
            .and_then(|t| t.parent_thread_id);
        for _ in 0..MAX_CHAIN {
            match next {
                Some(p) if p == ancestor => return Ok(true),
                Some(p) => {
                    next = self
                        .store(|s| s.thread(p))?
                        .and_then(|t| t.parent_thread_id)
                }
                None => return Ok(false),
            }
        }
        Ok(false)
    }

    /// How many delegations deep `thread` is: 0 for a user's own thread.
    fn depth(&self, thread: &Thread) -> Result<u32, String> {
        let mut depth = 0;
        let mut parent = thread.parent_thread_id;
        while let Some(id) = parent {
            depth += 1;
            if depth > MAX_CHAIN {
                break;
            }
            parent = self
                .store(|s| s.thread(id))?
                .and_then(|t| t.parent_thread_id);
        }
        Ok(depth)
    }

    fn read(&self, thread: &Thread, since_seq: i64) -> Result<Value, String> {
        let rows = self.store(|s| s.events_since(thread.id, since_seq))?;
        let last_seq = rows.last().map_or(since_seq, |e| e.seq);
        let mut text = Vec::new();
        let mut result = Value::Null;
        for row in &rows {
            match Record::from_row(&row.kind, &row.payload_json) {
                Ok(Record::Agent(Event::Assistant { content })) => {
                    text.extend(content.into_iter().filter_map(|b| match b {
                        Block::Text { text } => Some(text),
                        _ => None,
                    }))
                }
                Ok(Record::Agent(Event::Result {
                    is_error,
                    duration_ms,
                    result: r,
                    ..
                })) => {
                    result = json!({"is_error": is_error, "duration_ms": duration_ms, "result": r})
                }
                _ => {}
            }
        }
        let mut text = text.join("\n\n");
        if text.len() > MAX_READ {
            let cut = text.ceil_char_boundary(text.len() - MAX_READ);
            text = format!("…{}", &text[cut..]);
        }
        Ok(json!({
            "thread_id": thread.id,
            "status": thread.status,
            "last_seq": last_seq,
            "text": text,
            "result": result,
        }))
    }
}

use skyelab_core::delegation::is_busy as busy;

impl Host for AppHost {
    fn call(&self, caller: &Caller, tool: Tool) -> Result<Value, String> {
        // The grant names the caller's thread and account; both must still agree with the store.
        let me = self
            .store(|s| s.thread(caller.thread_id))?
            .filter(|t| t.account_id == caller.account_id)
            .ok_or("this session's thread is gone or ran under another account")?;
        match tool {
            Tool::ListProjects => {
                let config = self.config()?;
                let projects = self.store(|s| s.list_projects())?;
                let mine: Vec<Value> = projects
                    .iter()
                    .filter(|p| {
                        config
                            .delegation_account(&caller.account_id, &p.path)
                            .is_ok()
                    })
                    .map(|p| json!({"id": p.id, "name": p.name, "path": p.path}))
                    .collect();
                Ok(Value::Array(mine))
            }
            Tool::StartThread {
                project_id,
                prompt,
                worktree,
            } => {
                let depth = self.depth(&me)?;
                let live_children = self
                    .store(|s| s.child_threads(me.id))?
                    .iter()
                    .filter(|t| busy(&t.status))
                    .count();
                skyelab_core::delegation::check_limits(depth, live_children)?;
                let request = StartRequest {
                    caller: caller.clone(),
                    depth,
                    live_children,
                    project_id,
                    prompt,
                    worktree,
                };
                let id = ask(&self.calls, |reply| OrchCall::Start { request, reply })?;
                Ok(json!({"thread_id": id}))
            }
            Tool::ThreadStatus { thread_id } => {
                let t = self.drivable(caller, thread_id)?;
                Ok(json!({
                    "thread_id": t.id,
                    "title": t.title,
                    "status": t.status,
                    "account_id": t.account_id,
                    "project_id": t.project_id,
                    "parent_thread_id": t.parent_thread_id,
                }))
            }
            Tool::ReadThread {
                thread_id,
                since_seq,
            } => {
                let t = self.drivable(caller, thread_id)?;
                self.read(&t, since_seq)
            }
            Tool::SendMessage { thread_id, text } => {
                if thread_id == caller.thread_id {
                    return Err("a thread can't send messages to itself".into());
                }
                self.drivable(caller, thread_id)?;
                ask(&self.calls, |reply| OrchCall::Send {
                    caller: caller.clone(),
                    thread_id,
                    text,
                    reply,
                })?;
                Ok(json!({"ok": true}))
            }
            Tool::Browser(tool) => ask(&self.calls, |reply| OrchCall::Browser {
                caller: caller.clone(),
                tool,
                reply,
            }),
            Tool::WaitThread {
                thread_id,
                timeout_s,
            } => {
                let deadline = Instant::now() + Duration::from_secs(timeout_s);
                loop {
                    let t = self.drivable(caller, thread_id)?;
                    let done = !busy(&t.status);
                    if done || Instant::now() >= deadline {
                        return Ok(json!({"thread_id": t.id, "status": t.status, "done": done}));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }
}

pub use skyelab_agent::mcp::Grant;

/// What `launch()` needs to give a session the tools.
#[derive(Clone)]
pub struct Tools {
    server: Arc<Server>,
    proxy: Proxy,
}

impl Tools {
    pub fn get(cx: &App) -> Option<Tools> {
        cx.try_global::<Orchestration>().map(|o| Tools {
            server: o.server.clone(),
            proxy: o.proxy.clone(),
        })
    }

    /// Best effort: without a grant the agent still runs, just without SkyeLab's tools.
    pub fn grant(&self, thread_id: i64, account_id: &str) -> Option<Grant> {
        let caller = Caller {
            thread_id,
            account_id: account_id.to_owned(),
        };
        self.server
            .grant(caller, &self.proxy)
            .map_err(|e| {
                eprintln!("skyelab: thread {thread_id} runs without orchestration tools: {e:#}")
            })
            .ok()
    }
}
