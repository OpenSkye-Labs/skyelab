//! A thread tab: a prompt, and the agent session it starts under the folder's bound account.
//!
//! Store writes and every Session call run off the UI thread: SQLite is file I/O, and killing
//! reaps the child while holding the session lock.

use super::thread_model::{Item, Record, apply};
use super::{Backend, SharedStore, lock};
use anyhow::Result;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, SharedString, Subscription, Task, Window, actions, px,
};
use gpui_component::input::{InputEvent, TextareaState};
use skyelab_agent::Session;
use skyelab_core::AccountsConfig;
use skyelab_store::{Store, Thread};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

actions!(skyelab, [SendPrompt, Interrupt, Stop]);

pub(super) const CONTEXT: &str = "Thread";

/// The accounts.toml a launch re-reads right before it spawns.
pub struct AccountsFile(pub PathBuf);

impl gpui::Global for AccountsFile {}

#[path = "thread_launch.rs"]
mod launch;
#[path = "thread_saves.rs"]
mod saves;
#[path = "thread_session.rs"]
mod session;

pub enum ThreadViewEvent {
    /// The thread got its store row (the first prompt was sent).
    Started(Box<Thread>),
    /// The agent's Result event arrived: its changes may be on disk now.
    TurnDone,
    OpenChanges,
    /// A terminal in the thread's worktree, under the thread's account.
    OpenTerminal {
        cwd: PathBuf,
        account_id: String,
    },
}

/// A thread this one delegated, listed under its header.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildLink {
    pub id: i64,
    pub title: SharedString,
}

pub struct ThreadView {
    pub(super) focus: FocusHandle,
    pub(super) project_id: i64,
    pub(super) cwd: PathBuf,
    pub(super) config: AccountsConfig,
    pub(super) backend: Backend,
    pub(super) prompt: Entity<TextareaState>,
    pub(super) thread: Option<Thread>,
    pub(super) session: Option<Arc<Mutex<Session>>>,
    /// Prompts the session was sent that haven't had their result yet. The row says `idle` only at zero, so a
    /// follow-up queued behind a running turn keeps its child slot until that one is done too.
    pub(super) turns: Arc<std::sync::atomic::AtomicUsize>,
    /// The folder was unbound at send time; the view shows the bind call to action.
    pub(super) needs_binding: bool,
    pub(super) items: Vec<Item>,
    pub(super) list: ListState,
    expanded: HashSet<String>,
    pub(super) error: Option<SharedString>,
    pub(super) starting: bool,
    stop_requested: bool,
    /// Bumped by every launch; a pump from an earlier launch never ends a later one.
    pub(super) launches: u64,
    /// Nothing to send to: the agent never reported a session id to resume, or a reopened
    /// thread's row is still loading.
    pub(super) read_only: bool,
    pub(super) wt: super::thread_worktree::WorktreeState,
    /// Why continuing this thread was refused (its folder no longer resolves to its account).
    pub(super) resume_refused: Option<SharedString>,
    /// Records the store refused, held for a retry, and how many there are (shown as a warning).
    unsaved: saves::Unsaved,
    pub(super) unsaved_events: usize,
    retrying_saves: bool,
    /// The thread whose agent started this one: (id, title).
    pub(super) parent: Option<(i64, SharedString)>,
    /// A delegated start runs only under the account its delegation was approved for.
    pub(super) required_account: Option<String>,
    pub(super) children: Vec<ChildLink>,
    /// This session's right to SkyeLab's MCP tools; dropped when the agent is gone.
    grant: Option<super::orchestrate::Grant>,
    send_chain: Option<Task<()>>,
    /// Answered with the next follow-up's result, once its store write (and slot claim) is done.
    pub(super) on_sent: Option<flume::Sender<Result<(), String>>>,
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
                    let parent = match row.as_ref().and_then(|t| t.parent_thread_id) {
                        Some(p) => lock(&store)?.thread(p)?.map(|t| (t.id, t.title.into())),
                        None => None,
                    };
                    let children: Vec<ChildLink> = lock(&store)?
                        .child_threads(id)?
                        .into_iter()
                        .map(|t| ChildLink {
                            id: t.id,
                            title: t.title.into(),
                        })
                        .collect();
                    let rows = lock(&store)?.events_since(id, 0)?;
                    // One unreadable row (an older format) must not hide the rest of the thread.
                    let records: Vec<_> = rows
                        .iter()
                        .filter_map(|e| Record::from_row(&e.kind, &e.payload_json).ok())
                        .collect();
                    anyhow::Ok((row, (parent, children), rows.len() - records.len(), records))
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok((row, (parent, children), skipped, records)) => {
                    if let Some(row) = row {
                        this.thread = Some(row);
                    }
                    this.parent = parent;
                    this.children = children;
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
            cx.on_app_quit(|this, _| this.quitting()),
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
            launches: 0,
            read_only: false,
            wt: Default::default(),
            resume_refused: None,
            unsaved: Default::default(),
            unsaved_events: 0,
            retrying_saves: false,
            turns: Arc::default(),
            parent: None,
            required_account: None,
            children: Vec::new(),
            grant: None,
            send_chain: None,
            on_sent: None,
            _pump: None,
            _task: None,
            _subs: subs,
        }
    }

    /// May start or drive other threads: its agent runs (or is starting) and the user hasn't stopped it.
    #[cfg(unix)]
    pub(super) fn is_live(&self) -> bool {
        (self.session.is_some() || self.starting) && !self.stop_requested
    }

    /// Accounts changed on disk (Settings saved). Used for the next start and the badge.
    pub fn set_config(&mut self, config: AccountsConfig, cx: &mut Context<Self>) {
        self.config = config;
        self.resume_refused = None; // the next send checks the new bindings again
        if self.needs_binding && self.config.resolve(&self.cwd).is_some() {
            self.needs_binding = false;
        }
        cx.notify();
    }

    /// A PR was opened from this thread (already stored on its row).
    pub fn set_pr_url(&mut self, url: String, cx: &mut Context<Self>) {
        if let Some(t) = self.thread.as_mut() {
            t.pr_url = Some(url);
            cx.notify();
        }
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

    /// The provider of the account the thread was started with (`None` before the thread has a row,
    /// or when that account is gone).
    pub fn provider(&self) -> Option<skyelab_core::Provider> {
        let id = &self.thread.as_ref()?.account_id;
        let account = self.config.accounts.iter().find(|a| &a.id == id)?;
        Some(account.provider)
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
        if let (Some(want), Some(got)) = (&self.required_account, &resolved)
            && got.id != *want
        {
            self.error =
                Some(format!("This folder is now bound to {}, not {want}.", got.id).into());
            cx.notify();
            return;
        }
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
}

impl Focusable for ThreadView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
