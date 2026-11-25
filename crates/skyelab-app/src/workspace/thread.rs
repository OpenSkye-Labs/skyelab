//! A thread tab: a prompt, and the agent session it starts under the folder's bound account.
//!
//! Store writes and every Session call run off the UI thread: SQLite is file I/O, and killing
//! reaps the child while holding the session lock.

use super::thread_find::FindHost as _;
use super::thread_model::{Item, Record, apply, limit_after, replay, stamp_done};
use super::{Backend, SharedStore, lock};
use anyhow::Result;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, SharedString,
    Subscription, Task, Window, actions,
};
use gpui_component::input::TextareaState;
use skyelab_agent::{Mode, Session};
use skyelab_core::AccountsConfig;
use skyelab_store::Thread;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

actions!(skyelab, [SendPrompt, Interrupt, Stop, RestartAgentSession]);

pub(super) const CONTEXT: &str = "Thread";

#[path = "thread_access.rs"]
mod access;
#[path = "thread_answers.rs"]
mod answers;
/// The composer under the thread (`composer.rs` and its parts).
#[path = "composer.rs"]
pub(super) mod composer;
#[path = "thread_follow_up.rs"]
mod follow_up;
#[path = "thread_launch.rs"]
mod launch;
#[path = "thread_mode.rs"]
mod mode;
#[path = "thread_release.rs"]
pub(super) mod release;
#[path = "thread_restart.rs"]
mod restart;
#[path = "thread_resume.rs"]
mod resume;
#[path = "thread_saves.rs"]
mod saves;
#[path = "thread_session.rs"]
mod session;
#[path = "thread_teardown.rs"]
mod teardown;
#[path = "thread_window.rs"]
mod window_owner;
pub(in crate::workspace) use window_owner::{PLACEHOLDER, new_list};

#[path = "thread_context.rs"]
pub(super) mod context_meter;
#[path = "thread_event.rs"]
mod event;
pub use super::thread_details::ChildLink;
pub use event::{GitAction, ThreadViewEvent};

pub struct ThreadView {
    /// Debug mode's live count (docs/debugging.md): a count that grows as threads close is a leak.
    _live: skyelab_core::debug::Live,
    pub(super) focus: FocusHandle,
    pub(super) project_id: i64,
    /// The project's name, as the sidebar shows it (the workspace keeps it current across renames).
    pub(super) project_name: SharedString,
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
    pub(super) items: super::thread_model::Items,
    /// `Some(resets_at)` while the account's usage limit holds the agent back.
    pub(super) limited: Option<Option<i64>>,
    pub(super) context: context_meter::Reading,
    /// The timeline's list state, one per window that shows the thread (`window_owner::Lists`).
    pub(super) lists: window_owner::Lists,
    expanded: HashSet<String>,
    /// Turn files, the find bar, a closed failure banner.
    pub(super) timeline: super::turn_files::Timeline,
    /// Unsent AskUserQuestion picks, by ask id.
    pub(super) drafts: std::collections::HashMap<String, super::ask_cards::QuestionDraft>,
    pub(super) error: Option<SharedString>,
    pub(super) starting: bool,
    /// When the running (or last) turn's prompt was sent, ms since the epoch: the "Working 1m 12s" timers.
    pub(super) turn_started_ms: Option<i64>,
    pub(super) _tick: Option<Task<()>>,
    /// Hooks the agent runs now, newest last.
    pub(super) hooks: Vec<super::thread_hooks::Hook>,
    /// The launch that hasn't reported its session or output yet: the working row names its step.
    pub(super) startup: Option<super::thread_hooks::Startup>,
    /// The prompt a resume launch sent, kept until its session answers: rerun in a new session if it was gone.
    pub(super) resumed_prompt: Option<String>,
    /// Images sent from this tab, by their message's shown text: in memory only, for the thumbnails and viewer.
    pub(super) sent_images: std::collections::HashMap<String, super::image_viewer::Sent>,
    /// Set when the resumed session was gone: rerun when this agent has ended.
    pub(super) rerun: Option<String>,
    /// Started without the user (delegated, or from another device): its row is marked unread when it gets one.
    pub(super) background: bool,
    /// The window that owns it (last showed it): its prompt lives there, and only it acts on the view's events.
    pub(super) window: gpui::AnyWindowHandle,
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
    /// Its owner window (`window`) shows another machine: actions that open a new local thread (Continue in …) can't
    /// run there.
    pub(in crate::workspace) remote_window: bool,
    /// Records the store refused, held for a retry, and how many there are (shown as a warning).
    unsaved: saves::Unsaved,
    pub(super) unsaved_events: usize,
    retrying_saves: bool,
    /// The thread whose agent started this one: (id, title).
    pub(super) parent: Option<(i64, SharedString)>,
    /// A delegated start runs only under the account its delegation was approved for.
    pub(super) required_account: Option<String>,
    /// The multi-model send this draft is one model of: written on its row when the launch saves it.
    pub(super) compare_group: Option<i64>,
    /// The scheduled task this new thread runs for: written on its row (the sidebar's badge).
    pub(super) scheduled_task: Option<i64>,
    pub(super) children: Vec<ChildLink>,
    /// The details card's state for this thread.
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
    /// The rows and minimap, drawn cached so a composer-only redraw (the caret blink) doesn't rebuild them.
    pub(super) timeline_view: Entity<super::timeline_view::TimelineView>,
    _pump: Option<Task<()>>,
    /// Ends the agent once it has sat idle a while (`thread_release.rs`).
    pub(super) release: release::Release,
    _task: Option<Task<()>>,
    pub(super) _subs: Vec<Subscription>,
    pub(super) _watch: Option<Task<()>>,
}

impl EventEmitter<ThreadViewEvent> for ThreadView {}

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
        this.project_name = project.name.clone().into();
        this.load_composer(cx);
        this.init_extras(window, cx);
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
        this.init_extras(window, cx);
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
                    let sides = lock(&store)?.side_chats(id)?;
                    let sides = sides.into_iter().map(|t| (t.id, t.title.into())).collect();
                    let lineage = (parent, children, sides);
                    let mark = lock(&store)?.scroll_mark(id)?;
                    anyhow::Ok((row, lineage, rows.len() - records.len(), records, mark))
                })
                .await;
            let _ = this.update(cx, |this, cx| match loaded {
                Ok((row, (parent, children, sides), skipped, records, mark)) => {
                    if let Some(row) = row {
                        this.mode = super::side_chat::row_mode(&row);
                        this.thread = Some(row);
                    }
                    this.parent = parent;
                    this.children = children;
                    this.details.side_chats = sides;
                    this.finish(cx);
                    let fork_made = this.thread.as_ref().filter(|t| t.forked_from.is_some());
                    let fork_made = fork_made.map(|t| t.created_at);
                    let _span = tracing::info_span!("thread.replay").entered();
                    for (r, at) in &records {
                        replay(&mut this.items, r, *at, fork_made);
                        this.limited = limit_after(this.limited, r);
                        this.note_model(r, false);
                        // A fork's copied rows are its source's processes, not its own.
                        if fork_made.is_none_or(|made| *at > made) {
                            super::thread_details::fold_mcp(&mut this.details.mcp, r);
                        }
                        this.note_context(r);
                    }
                    this.lists.reset(this.items.len());
                    this.lists.restore(mark);
                    crate::debug_mode::launch_milestone("thread replayed");
                    this.load_turn_files(cx);
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
        let prompt = window_owner::new_prompt(window, cx);
        let subs = vec![
            window_owner::prompt_subscription(&prompt, window, cx),
            cx.on_app_quit(|this, _| this.quitting()),
        ];
        let lists = window_owner::Lists::new(window.window_handle());
        let watch = super::folder_watch::watch(&cwd, cx, Self::folder_changed);
        let owner = cx.weak_entity();
        let branch_picker = cx.new(|cx| {
            super::branch_picker::BranchPicker::new(owner.clone(), cwd.clone(), window, cx)
        });
        let scripts = cx.new(|cx| {
            super::scripts::ScriptMenu::new(owner, backend.store.clone(), project_id, window, cx)
        });
        let this = cx.entity();
        let timeline_view = cx.new(|cx| super::timeline_view::TimelineView::new(&this, cx));
        Self {
            window: window.window_handle(),
            branch_picker,
            scripts,
            folder_account: config.resolve(&cwd).cloned(),
            _live: skyelab_core::debug::Live::new("thread views"),
            focus: cx.focus_handle(),
            project_id,
            // The folder's name until the workspace names the project (`new` and `open_stored` do at once).
            project_name: cwd.file_name().map_or_else(
                || cwd.display().to_string().into(),
                |n| n.to_string_lossy().into_owned().into(),
            ),
            cwd,
            config,
            backend,
            prompt,
            thread: None,
            session: None,
            needs_binding: false,
            items: Default::default(),
            limited: None,
            context: Default::default(),
            lists,
            expanded: HashSet::new(),
            timeline: Default::default(),
            drafts: Default::default(),
            error: None,
            starting: false,
            turn_started_ms: None,
            _tick: None,
            hooks: Vec::new(),
            startup: None,
            resumed_prompt: None,
            sent_images: Default::default(),
            rerun: None,
            background: false,
            stop_requested: false,
            launches: 0,
            read_only: false,
            wt: Default::default(),
            resume_refused: None,
            remote_window: false,
            unsaved: Default::default(),
            unsaved_events: 0,
            retrying_saves: false,
            turns: Arc::default(),
            parent: None,
            required_account: None,
            compare_group: None,
            scheduled_task: None,
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
            timeline_view,
            _pump: None,
            release: Default::default(),
            _task: None,
            _watch: watch,
            _subs: subs,
        }
    }

    /// May start or drive other threads: its agent runs (or is starting) and the user hasn't stopped it.
    pub(super) fn is_live(&self) -> bool {
        (self.session.is_some() || self.starting) && !self.stop_requested
    }

    pub(in crate::workspace) fn show_error(&mut self, e: anyhow::Error, cx: &mut Context<Self>) {
        self.error = Some(format!("{e:#}").into());
        cx.notify();
    }

    pub(super) fn push(&mut self, record: &Record, cx: &mut Context<Self>) {
        // A prompt with no turn running starts one (a steer or queued send joins it).
        if matches!(record, Record::Prompt(_) | Record::Steer(_)) {
            self.release.disarm();
        }
        if matches!(record, Record::Prompt(_)) && !self.turn_running() {
            self.turn_started_ms = Some(crate::ui::now_ms());
            self.tick_while_working(cx);
        }
        if let Record::Agent(skyelab_agent::Event::RateLimit { .. }) = record {
            cx.emit(ThreadViewEvent::UsageReported);
        }
        self.note_model(record, true);
        if matches!(record, Record::Agent(skyelab_agent::Event::Context { .. })) {
            self.note_context(record);
            cx.notify();
        }
        if super::thread_details::fold_mcp(&mut self.details.mcp, record) {
            cx.notify();
        }
        let limited = limit_after(self.limited, record);
        if limited != self.limited {
            self.limited = limited;
            cx.notify();
        }
        let old = self.items.len();
        if let Some(first) = apply(&mut self.items, record) {
            stamp_done(&mut self.items, crate::ui::now_ms());
            self.find_rows_changed();
            self.lists.splice(first..old, self.items.len() - first);
            self.lists.reveal_ask(record);
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
