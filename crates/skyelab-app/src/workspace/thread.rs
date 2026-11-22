//! A thread tab: a prompt, and the agent session it starts under the folder's bound account.
//!
//! Store writes and every Session call run on the background executor: a stdin write blocks
//! when `claude` stops reading, and SQLite is file I/O. The UI thread only applies results.

use super::Backend;
use super::thread_model::{Item, Record, apply};
use anyhow::{Context as _, Result};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, SharedString, Subscription, Task, Window, actions, px,
};
use gpui_component::input::{InputEvent, TextareaState};
use skyelab_agent::{Event, Session};
use skyelab_core::AccountsConfig;
use skyelab_store::{NewThread, Store, Thread};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

actions!(skyelab, [SendPrompt, Interrupt, Stop]);

pub(super) const CONTEXT: &str = "Thread";

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
    stopping: bool,
    /// Reopened from the store: shown, not continued (skyelab-agent can't resume a session yet).
    pub(super) past: bool,
    _pump: Option<Task<()>>,
    _task: Option<Task<()>>,
    _subs: Vec<Subscription>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

fn lock<T>(m: &Mutex<T>) -> Result<MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| anyhow::anyhow!("a background task panicked holding the lock"))
}

fn append(store: &mut Store, thread_id: i64, record: &Record) -> Result<()> {
    let (kind, payload) = record.to_row();
    store.append_event(thread_id, kind, &payload).map(|_| ())
}

/// The first line of the prompt, cut to a tab-sized title.
fn title(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or_default().trim();
    let mut t: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        t.push('…');
    }
    t
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
        this.past = true;
        this.prompt.update(cx, |p, cx| p.set_disabled(true, cx));
        let (store, id) = (this.backend.store.clone(), thread.id);
        this.thread = Some(thread);
        this._task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_spawn(async move {
                    let rows = lock(&store)?.events_since(id, 0)?;
                    // One unreadable row (an older format) must not hide the rest of the thread.
                    let records: Vec<_> = rows
                        .iter()
                        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
                        .collect();
                    anyhow::Ok((rows.len() - records.len(), records))
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok((skipped, records)) => {
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
            stopping: false,
            past: false,
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
        if text.is_empty() || self.past || self.starting {
            return;
        }
        if self.session.is_some() {
            self.follow_up(text, window, cx);
            return;
        }
        // The folder decides the account. Unbound means ask the user, never pick one.
        let Some(account) = self.config.resolve(&self.cwd).cloned() else {
            self.needs_binding = true;
            cx.notify();
            return;
        };
        self.needs_binding = false;
        self.error = None;
        self.starting = true;
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        self.push(&Record::Prompt(text.clone()), cx);

        let (store, bin, config, cwd, project_id) = (
            self.backend.store.clone(),
            self.backend.claude_bin.clone(),
            self.config.clone(),
            self.cwd.clone(),
            self.project_id,
        );
        let pump_store = store.clone();
        self._task = Some(cx.spawn(async move |this, cx| {
            let started = cx
                .background_spawn(async move {
                    let thread = lock(&store)?.create_thread(&NewThread {
                        project_id,
                        title: &title(&text),
                        cwd: &cwd,
                        account_id: &account.id,
                        status: "starting",
                    })?;
                    append(
                        &mut *lock(&store)?,
                        thread.id,
                        &Record::Prompt(text.clone()),
                    )?;
                    let session = skyelab_agent::start_with_bin(&bin, &config, &cwd, &text);
                    let status = if session.is_ok() { "running" } else { "failed" };
                    lock(&store)?.set_thread_status(thread.id, status)?;
                    anyhow::Ok((thread, session))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.starting = false;
                match started {
                    Ok((thread, session)) => {
                        this.thread = Some(thread.clone());
                        cx.emit(ThreadViewEvent::Started(thread.clone()));
                        match session {
                            Ok(session) => this.run(session, pump_store, thread.id, cx),
                            Err(e) => this.show_error(e.context("starting the agent"), cx),
                        }
                    }
                    Err(e) => this.show_error(e, cx),
                }
                cx.notify();
            });
        }));
    }

    fn follow_up(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        let record = Record::Prompt(text.clone());
        self.push(&record, cx);
        self.in_background(cx, move || {
            append(&mut *lock(&store)?, id, &record)?;
            lock(&store)?.set_thread_status(id, "running")?;
            lock(&session)?.send_user_message(&text)
        });
    }

    /// Streams the session's events: each is stored first, then shown, so the store keeps their order.
    fn run(&mut self, session: Session, store: Arc<Mutex<Store>>, id: i64, cx: &mut Context<Self>) {
        let events = session.events().clone();
        self.session = Some(Arc::new(Mutex::new(session)));
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
                    this.push(&record, cx);
                });
                if shown.is_err() {
                    return;
                }
            }
            // stdout closed: claude and everything it started are gone.
            let _ = this.update(cx, |this, cx| this.ended(cx));
        }));
    }

    fn ended(&mut self, cx: &mut Context<Self>) {
        if self.stopping {
            return;
        }
        let (Some(session), Some(thread)) = (self.session.take(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        // Dropping the session reaps the child, which waits; keep that off the UI thread.
        self.in_background(cx, move || {
            drop(session);
            lock(&store)?.set_thread_status(id, "exited")
        });
        cx.notify();
    }

    pub(super) fn interrupt(&mut self, _: &Interrupt, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(session) = self.session.clone() {
            self.in_background(cx, move || lock(&session)?.interrupt());
        }
    }

    pub(super) fn stop(&mut self, _: &Stop, _: &mut Window, cx: &mut Context<Self>) {
        let (Some(session), Some(thread)) = (self.session.clone(), &self.thread) else {
            return;
        };
        let (store, id) = (self.backend.store.clone(), thread.id);
        self.stopping = true;
        self._task = Some(cx.spawn(async move |this, cx| {
            // shortcut: a stdin write blocked on a stuck claude holds this lock, so Stop waits for it; split stdin from kill in skyelab-agent if that shows up.
            let stopped = cx
                .background_spawn(async move {
                    lock(&session)?.kill().context("stopping the agent")?;
                    lock(&store)?.set_thread_status(id, "stopped")
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.stopping = false;
                if let Err(e) = stopped {
                    return this.show_error(e, cx);
                }
                this._pump = None;
                this.session = None; // already killed and reaped, so dropping it is instant
                cx.notify();
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
        if let Some(session) = self.session.take() {
            let _ = std::thread::Builder::new()
                .name("skyelab-agent-reaper".into())
                .spawn(move || drop(session));
        }
    }
}
