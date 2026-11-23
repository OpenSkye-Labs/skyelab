//! A thread tab: a prompt, and the agent session it starts under the folder's bound account.
//!
//! Store writes and every Session call run off the UI thread: SQLite is file I/O, and killing
//! reaps the child while holding the session lock.

use super::thread_model::{Item, Record, apply, limit_after, stamp_done};
use super::{Backend, SharedStore, lock};
use anyhow::Result;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment,
    ListState, SharedString, Subscription, Task, Window, actions, px,
};
use gpui_component::input::{InputEvent, TextareaState};
use skyelab_agent::{Mode, Session};
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

/// The accounts as `file` says right now (`fallback` without one). An unreadable file binds nothing.
pub(super) fn accounts_now(
    file: Option<&std::path::Path>,
    fallback: &skyelab_core::AccountsConfig,
) -> skyelab_core::AccountsConfig {
    file.map_or_else(
        || fallback.clone(),
        |p| skyelab_core::AccountsConfig::load(p).unwrap_or_default(),
    )
}

#[path = "thread_answers.rs"]
mod answers;
/// The composer under the thread (`composer.rs` and its parts).
#[path = "composer.rs"]
pub(super) mod composer;
#[path = "thread_follow_up.rs"]
mod follow_up;
#[path = "thread_launch.rs"]
mod launch;
#[path = "thread_saves.rs"]
mod saves;
#[path = "thread_session.rs"]
mod session;

#[path = "thread_event.rs"]
mod event;
pub use super::thread_details::ChildLink;
pub use event::{GitAction, ThreadViewEvent};

pub struct ThreadView {
    pub(super) focus: FocusHandle,
    pub(super) project_id: i64,
    pub(super) cwd: PathBuf,
    pub(super) config: AccountsConfig,
    /// The account `cwd` resolves to now, for display (resolving touches the filesystem, so never in render).
    pub(super) folder_account: Option<skyelab_core::Account>,
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
    /// `Some(resets_at)` while the account's usage limit holds the agent back.
    pub(super) limited: Option<Option<i64>>,
    pub(super) list: ListState,
    expanded: HashSet<String>,
    /// Unsent AskUserQuestion picks, by ask id.
    pub(super) drafts: std::collections::HashMap<String, super::ask_cards::QuestionDraft>,
    pub(super) error: Option<SharedString>,
    pub(super) starting: bool,
    /// When the running (or last) turn's prompt was sent, ms since the epoch: the "Working 1m 12s" timers.
    pub(super) turn_started_ms: Option<i64>,
    pub(super) _tick: Option<Task<()>>,
    /// The prompt a resume launch sent, kept until its session answers: rerun in a new session if it was gone.
    pub(super) resumed_prompt: Option<String>,
    /// Set when the resumed session was gone: rerun when this agent has ended.
    pub(super) rerun: Option<String>,
    /// Started without the user (delegated, or from another device): its row is marked unread when it gets one.
    pub(super) background: bool,
    stop_requested: bool,
    /// Bumped by every launch; a pump from an earlier launch never ends a later one.
    pub(super) launches: u64,
    /// Nothing to send to: the agent never reported a session id to resume, or a reopened
    /// thread's row is still loading.
    pub(super) read_only: bool,
    pub(super) wt: super::thread_worktree::WorktreeState,
    /// The header's branch picker, on the folder this view opened on.
    pub(super) branch_picker: Entity<super::branch_picker::BranchPicker>,
    /// The header's Run menu: the project's scripts.
    pub(super) scripts: Entity<super::scripts::ScriptMenu>,
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
    /// The Details tab's state for this thread.
    pub(super) details: super::thread_details::Details,
    /// Which tool calls the agent makes without asking. Stored on the row, used at every launch.
    pub(super) mode: Mode,
    /// Full access was picked and waits for the user to confirm it.
    pub(super) confirm_bypass: bool,
    /// Mode writes (store row, running agent) run one at a time, each writing the newest pick, so two quick picks
    /// can't land in the wrong order.
    mode_writes: Arc<Mutex<()>>,
    mode_now: Arc<std::sync::atomic::AtomicUsize>,
    /// This session's right to SkyeLab's MCP tools; dropped when the agent is gone.
    grant: Option<super::orchestrate::Grant>,
    pub(super) send_chain: Option<Task<()>>,
    /// Answered with the next follow-up's result, once its store write (and slot claim) is done.
    pub(super) on_sent: Option<flume::Sender<Result<(), String>>>,
    /// Attachments, menus, the model pick, the queue and the footer (`composer_state.rs`).
    pub(super) composer: composer::Composer,
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
        let mut this = Self::build(
            project.id,
            project.path.clone(),
            config,
            backend,
            window,
            cx,
        );
        this.load_composer(cx);
        this
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
        this.load_composer(cx);
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
                            status: crate::ui::Status::parse(&t.status),
                            started_ms: t.created_at,
                            title: t.title.into(),
                        })
                        .collect();
                    let rows = lock(&store)?.events_since(id, 0)?;
                    // One unreadable row (an older format) must not hide the rest of the thread.
                    let records: Vec<_> = rows
                        .iter()
                        .filter_map(|e| {
                            Some((
                                Record::from_row(&e.kind, &e.payload_json).ok()?,
                                e.created_at,
                            ))
                        })
                        .collect();
                    anyhow::Ok((row, (parent, children), rows.len() - records.len(), records))
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok((row, (parent, children), skipped, records)) => {
                    if let Some(row) = row {
                        this.mode = Mode::parse(&row.permission_mode);
                        this.thread = Some(row);
                    }
                    this.parent = parent;
                    this.children = children;
                    this.finish(cx);
                    for (r, at) in &records {
                        apply(&mut this.items, r);
                        stamp_done(&mut this.items, *at);
                        this.limited = limit_after(this.limited, r);
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
                .placeholder("Ask anything. @ files, $ skills, / commands")
        });
        let subs = vec![
            cx.subscribe_in(
                &prompt,
                window,
                |this, _, e: &InputEvent, window, cx| match e {
                    InputEvent::PressEnter {
                        secondary: true, ..
                    } => this.send_prompt(&SendPrompt, window, cx),
                    InputEvent::Change => this.prompt_changed(cx),
                    _ => {}
                },
            ),
            cx.on_app_quit(|this, _| this.quitting()),
        ];
        let list = ListState::new(0, ListAlignment::Top, px(600.));
        let owner = cx.weak_entity();
        let branch_picker = cx.new(|cx| {
            super::branch_picker::BranchPicker::new(owner.clone(), cwd.clone(), window, cx)
        });
        let scripts = cx.new(|cx| {
            super::scripts::ScriptMenu::new(owner, backend.store.clone(), project_id, window, cx)
        });
        Self {
            branch_picker,
            scripts,
            folder_account: config.resolve(&cwd).cloned(),
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
            limited: None,
            list,
            expanded: HashSet::new(),
            drafts: Default::default(),
            error: None,
            starting: false,
            turn_started_ms: None,
            _tick: None,
            resumed_prompt: None,
            rerun: None,
            background: false,
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
            details: Default::default(),
            mode: Mode::Default,
            confirm_bypass: false,
            mode_writes: Arc::default(),
            mode_now: Arc::default(),
            grant: None,
            send_chain: None,
            on_sent: None,
            composer: Default::default(),
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
        self.folder_account = self.config.resolve(&self.cwd).cloned();
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
    #[cfg(all(test, unix))] // only the unix-only thread tests read it
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
    #[cfg(all(test, unix))]
    pub fn provider(&self) -> Option<skyelab_core::Provider> {
        let id = &self.thread.as_ref()?.account_id;
        let account = self.config.accounts.iter().find(|a| &a.id == id)?;
        Some(account.provider)
    }

    /// What the sidebar shows for this thread: its live session's state, or with no session, stopped (the stored row
    /// may still say running from before).
    pub fn is_expanded(&self, tool_id: &str) -> bool {
        self.expanded.contains(tool_id)
    }

    pub fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(tool_id) {
            self.expanded.insert(tool_id.to_owned());
        }
        if let Some(rows) = super::timeline_fold::rows_of(&self.items, tool_id) {
            let n = rows.len();
            self.list.splice(rows, n);
        }
        cx.notify();
    }

    /// Picks the thread's permission mode. Full access waits for [`Self::confirm_mode`].
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if mode == Mode::Bypass && self.mode != Mode::Bypass {
            self.confirm_bypass = true;
            return cx.notify();
        }
        self.apply_mode(mode, cx);
    }

    /// The user confirmed Full access (or turned it down).
    pub fn confirm_mode(&mut self, yes: bool, cx: &mut Context<Self>) {
        let asked = std::mem::take(&mut self.confirm_bypass);
        if yes && asked {
            self.apply_mode(Mode::Bypass, cx);
        }
        cx.notify();
    }

    fn apply_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.confirm_bypass = false;
        if self.mode == mode {
            return cx.notify();
        }
        self.mode = mode;
        self.write_mode(cx);
        cx.notify();
    }

    /// Sends the newest mode to the running agent, then to the store row.
    pub(super) fn write_mode(&mut self, cx: &mut Context<Self>) {
        let ix = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        self.mode_now.store(ix, std::sync::atomic::Ordering::SeqCst);
        let row = self.thread.as_mut().map(|t| {
            t.permission_mode = self.mode.as_str().to_owned();
            t.id
        });
        let (store, session) = (self.backend.store.clone(), self.session.clone());
        let (writes, now) = (self.mode_writes.clone(), self.mode_now.clone());
        self.in_background(cx, move || {
            let _one_at_a_time = lock(&writes)?;
            let mode = Mode::ALL[now.load(std::sync::atomic::Ordering::SeqCst)];
            // Each step runs even if the other fails, so a store error never leaves the agent on the old mode.
            let live = session.map_or(Ok(()), |s| lock(&s)?.set_mode(mode));
            let stored = row.map_or(Ok(()), |id| {
                lock(&store)?.set_thread_permission_mode(id, mode.as_str())
            });
            live.and(stored)
        });
    }

    fn show_error(&mut self, e: anyhow::Error, cx: &mut Context<Self>) {
        self.error = Some(format!("{e:#}").into());
        cx.notify();
    }

    fn push(&mut self, record: &Record, cx: &mut Context<Self>) {
        // A prompt with no turn running starts one (a steer or queued send joins it).
        if matches!(record, Record::Prompt(_)) && !self.turn_running() {
            self.turn_started_ms = Some(crate::ui::now_ms());
            self.tick_while_working(cx);
        }
        if let Record::Agent(skyelab_agent::Event::RateLimit { .. }) = record {
            cx.emit(ThreadViewEvent::UsageReported);
        }
        let limited = limit_after(self.limited, record);
        if limited != self.limited {
            self.limited = limited;
            cx.notify();
        }
        let old = self.items.len();
        if let Some(first) = apply(&mut self.items, record) {
            stamp_done(&mut self.items, crate::ui::now_ms());
            self.list.splice(first..old, self.items.len() - first);
            cx.notify();
        }
    }

    /// An agent is running or starting in this thread: its folder's files and checkout must not change under it.
    pub(super) fn agent_running(&self) -> bool {
        self.session.is_some() || self.starting
    }

    pub(super) fn send_prompt(
        &mut self,
        _: &SendPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(composer::Submit::Send, window, cx);
    }

    pub(super) fn steer_prompt(
        &mut self,
        _: &composer::SteerPrompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit(composer::Submit::Steer, window, cx);
    }
}

impl Focusable for ThreadView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
