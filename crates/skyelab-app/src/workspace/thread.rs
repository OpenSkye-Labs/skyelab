//! A thread tab: a prompt, and the agent session it starts under the folder's bound account.
//!
//! Store writes and the blocking Session calls (start, kill) run off the UI thread: killing
//! reaps the child, and SQLite is file I/O. Stdin writes only queue, so they never block.

use super::thread_model::{Item, Record, apply};
use super::{Backend, SharedStore, lock};
use anyhow::{Context as _, Result};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, SharedString, Subscription, Task, Window, actions, px,
};
use gpui_component::input::{InputEvent, TextareaState};
use skyelab_agent::{Event, Session};
use skyelab_core::AccountsConfig;
use skyelab_store::{Store, Thread};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

actions!(skyelab, [SendPrompt, Interrupt, Stop]);

pub(super) const CONTEXT: &str = "Thread";

#[path = "thread_launch.rs"]
mod launch;

pub enum ThreadViewEvent {
    /// The thread got its store row (the first prompt was sent).
    Started(Thread),
}

pub struct ThreadView {
    pub(super) focus: FocusHandle,
    project_id: i64,
    pub(super) cwd: PathBuf,
    config: AccountsConfig,
    backend: Backend,
    pub(super) prompt: Entity<TextareaState>,
    pub(super) thread: Option<Thread>,
    pub(super) session: Option<Arc<Mutex<Session>>>,
    /// The folder was unbound at send time; the view shows the bind call to action.
    pub(super) needs_binding: bool,
    pub(super) items: Vec<Item>,
    pub(super) list: ListState,
    expanded: HashSet<String>,
    pub(super) error: Option<SharedString>,
    pub(super) starting: bool,
    stop_requested: bool,
    /// Nothing to send to: the agent never reported a session id to resume, or a reopened
    /// thread's row is still loading.
    pub(super) read_only: bool,
    /// Why continuing this thread was refused (its folder no longer resolves to its account).
    pub(super) resume_refused: Option<SharedString>,
    send_chain: Option<Task<()>>,
    _pump: Option<Task<()>>,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

fn append(store: &mut Store, thread_id: i64, record: &Record) -> Result<()> {
    let (kind, payload) = record.to_row();
    store.append_event(thread_id, kind, &payload).map(|_| ())
}

impl ThreadView {
    pub fn new(
        project: &skyelab_store::Project,
        config: AccountsConfig,
        backend: Backend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::build(
            project.id,
            project.path.clone(),
            config,
            backend,
            window,
            cx,
        )
    }

    /// Reopens a stored thread and replays its events.
    pub fn open(
        thread: Thread,
        config: AccountsConfig,
        backend: Backend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self::build(
            thread.project_id,
            thread.cwd.clone(),
            config,
            backend,
            window,
            cx,
        );
        let (store, id) = (this.backend.store.clone(), thread.id);
        this.thread = Some(thread);
        // Read-only until the fresh row below says whether there's a session to resume.
        this.read_only = true;
        this.prompt.update(cx, |p, cx| p.set_disabled(true, cx));
        this._task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    // The sidebar's copy can predate the init event that stored the session id.
                    let row = lock(&store)?.thread(id)?;
                    let rows = lock(&store)?.events_since(id, 0)?;
                    // One unreadable row (an older format) must not hide the rest of the thread.
                    let records: Vec<_> = rows
                        .iter()
                        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
                        .collect();
                    anyhow::Ok((row, rows.len() - records.len(), records))
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok((row, skipped, records)) => {
                    if let Some(row) = row {
                        this.thread = Some(row);
                    }
                    this.finish(cx);
                    for r in &records {
                        apply(&mut this.items, r);
                    }
                    this.list.reset(this.items.len());
                    if skipped > 0 {
                        this.error =
                            Some(format!("{skipped} stored events couldn't be read").into());
                    }
                    cx.notify();
                }
                Err(e) => this.show_error(e, cx),
            });
        }));
        this
    }

    fn build(
        project_id: i64,
        cwd: PathBuf,
        config: AccountsConfig,
        backend: Backend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 10)
                .placeholder("Ask the agent… (⌘↩ to send)")
        });
        let subs = vec![
            cx.subscribe_in(&prompt, window, |this, _, e: &InputEvent, window, cx| {
                if let InputEvent::PressEnter {
                    secondary: true, ..
                } = e
                {
                    this.send_prompt(&SendPrompt, window, cx);
                }
            }),
        ];
        let list = ListState::new(0, ListAlignment::Top, px(600.));
        Self {
            focus: cx.focus_handle(),
            project_id,
            cwd,
            config,
            backend,
            prompt,
            thread: None,
            session: None,
            needs_binding: false,
            items: Vec::new(),
            list,
            expanded: HashSet::new(),
            error: None,
            starting: false,
            stop_requested: false,
            read_only: false,
            resume_refused: None,
            send_chain: None,
            _pump: None,
            _task: None,
            _subs: subs,
        }
    }

    /// Accounts changed on disk (Settings saved). Used for the next start and the badge.
    pub fn set_config(&mut self, config: AccountsConfig, cx: &mut Context<Self>) {
        self.config = config;
        if self.needs_binding && self.config.resolve(&self.cwd).is_some() {
            self.needs_binding = false;
        }
        cx.notify();
    }

    pub fn focus_prompt(&self, window: &mut Window, cx: &mut App) {
        self.prompt.read(cx).focus_handle(cx).focus(window, cx);
    }

    pub fn title(&self) -> SharedString {
        match &self.thread {
            Some(t) => t.title.clone().into(),
            None => "New thread".into(),
        }
    }

    /// "Label · id" of the account the thread was started with, from its store row.
    pub fn badge(&self) -> Option<SharedString> {
        let id = &self.thread.as_ref()?.account_id;
        let label = self
            .config
            .accounts
            .iter()
            .find(|a| &a.id == id)
            .map_or("Unknown account", |a| a.label.as_str());
        Some(format!("{label} · {id}").into())
    }

    pub fn is_expanded(&self, tool_id: &str) -> bool {
        self.expanded.contains(tool_id)
    }

    pub fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(tool_id) {
            self.expanded.insert(tool_id.to_owned());
        }
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| matches!(i, Item::Tool(t) if t.id == tool_id))
        {
            self.list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }

    fn show_error(&mut self, e: anyhow::Error, cx: &mut Context<Self>) {
        self.error = Some(format!("{e:#}").into());
        cx.notify();
    }

    fn push(&mut self, record: &Record, cx: &mut Context<Self>) {
        let old = self.items.len();
        if let Some(first) = apply(&mut self.items, record) {
            self.list.splice(first..old, self.items.len() - first);
            cx.notify();
        }
    }

    pub(super) fn send_prompt(
        &mut self,
        _: &SendPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self.prompt.read(cx).value().trim().to_string();
        if text.is_empty() || self.read_only || self.starting {
            return;
        }
        // The folder decides the account. Unbound means ask the user, never pick one.
        let resolved = self.config.resolve(&self.cwd).cloned();
        if self.session.is_none()
            && let Some(thread) = self.thread.clone()
        {
            return self.resume(text, thread, resolved, window, cx);
        }
        let Some(account) = resolved else {
            self.needs_binding = true;
            cx.notify();
            return;
        };
        self.needs_binding = false;
        self.error = None;
        if self.session.is_some() {
            self.follow_up(text, &account, window, cx);
            return;
        }
        self.launch(text, account, None, window, cx);
    }

    /// Each event is stored before it is shown, so the store keeps stdout's order.
    fn run(&mut self, session: Session, store: SharedStore, id: i64, cx: &mut Context<Self>) {
        let events = session.events().clone();
        let session = Arc::new(Mutex::new(session));
        let mine = Arc::downgrade(&session);
        self.session = Some(session);
        self._pump = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv_async().await {
                let record = Record::Agent(event);
                let store = store.clone();
                let saved_record = record.clone();
                let saved = cx
                    .background_spawn(async move {
                        let mut store = lock(&store)?;
                        append(&mut store, id, &saved_record)?;
                        match &saved_record {
                            Record::Agent(Event::Init { session_id, .. }) => {
                                store.set_agent_session_id(id, Some(session_id))?
                            }
                            Record::Agent(Event::Result { .. }) => {
                                store.set_thread_status(id, "idle")?
                            }
                            _ => {}
                        }
                        anyhow::Ok(())
                    })
                    .await;
                let shown = this.update(cx, |this, cx| {
                    if let Err(e) = saved {
                        this.show_error(e.context("saving an agent event"), cx);
                    }
                    if let (Record::Agent(Event::Init { session_id, .. }), Some(t)) =
                        (&record, &mut this.thread)
                    {
                        t.agent_session_id = Some(session_id.clone());
                    }
                    this.push(&record, cx);
                });
                if shown.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| this.ended(&mine, cx));
        }));
    }

    /// claude's stdout closed and every event is stored: record how the thread ended.
    fn ended(&mut self, mine: &Weak<Mutex<Session>>, cx: &mut Context<Self>) {
        // After a Stop, a resumed session may already be starting or running: leave it alone.
        let replaced = self
            .session
            .as_ref()
            .is_some_and(|s| Arc::as_ptr(s) != mine.as_ptr());
        if self.starting || replaced {
            return;
        }
        let Some(thread) = &self.thread else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        let status = if self.stop_requested {
            "stopped"
        } else {
            "exited"
        };
        let session = self.session.take();
        self.finish(cx);
        // Reaping the child waits, so the session is dropped off the UI thread.
        self.in_background(cx, move || {
            drop(session);
            lock(&store)?.set_thread_status(id, status)
        });
    }

    /// The agent is gone. The next prompt resumes its session, if it reported one.
    fn finish(&mut self, cx: &mut Context<Self>) {
        self.read_only = self
            .thread
            .as_ref()
            .is_none_or(|t| t.agent_session_id.is_none());
        let disabled = self.read_only;
        self.prompt.update(cx, |p, cx| p.set_disabled(disabled, cx));
        cx.notify();
    }

    pub(super) fn interrupt(&mut self, _: &Interrupt, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.session.clone() {
            self.in_background(cx, move || lock(&session)?.interrupt());
        }
    }

    pub(super) fn stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        self.stop_requested = true;
        // While starting, the start task sees the flag and kills the new session.
        if !self.starting {
            self.kill(cx);
        }
        cx.notify();
    }

    /// Kills the agent. The pump keeps storing what it already printed, then `ended` runs.
    fn kill(&mut self, cx: &mut Context<Self>) {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        self._task = Some(cx.spawn(async move |this, cx| {
            let stopped = cx
                .background_spawn(async move {
                    lock(&session)?.kill().context("stopping the agent")?;
                    lock(&store)?.set_thread_status(id, "stopped")
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(e) = stopped {
                    return this.show_error(e, cx);
                }
                this.session = None; // killed and reaped, so dropping it doesn't wait
                this.finish(cx);
            });
        }));
    }

    /// Runs blocking work on the background executor and shows its error, if any, inline.
    fn in_background(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce() -> Result<()> + Send + 'static,
    ) {
        let task = cx.background_spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            if let Err(e) = task.await {
                let _ = this.update(cx, |this, cx| this.show_error(e, cx));
            }
        })
        .detach();
    }
}

impl Focusable for ThreadView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Drop for ThreadView {
    /// Closing the tab ends its agent. Killing and reaping waits, so it happens off the UI thread.
    fn drop(&mut self) {
        let (Some(session), Some(thread)) = (self.session.take(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        let _ = std::thread::Builder::new()
            .name("skyelab-agent-reaper".into())
            .spawn(move || {
                drop(session);
                if let Ok(store) = lock(&store) {
                    let _ = store.set_thread_status(id, "stopped");
                }
            });
    }
}
