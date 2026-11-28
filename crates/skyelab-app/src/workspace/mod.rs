//! The main window: sidebar, the center's one page, bottom terminals and a status bar, on `ui::dock`.

mod attention;
pub(crate) mod background;
mod bots_page;
mod branch_picker;
mod browser_agent;
mod browser_tabs;
mod center;
mod center_skin;
mod changes;
mod changes_generate;
mod changes_hooks;
mod changes_model;
mod changes_open;
mod changes_publish;
mod changes_tree;
mod changes_turns;
mod child_slots;
mod compare;
mod compare_sidebar;
mod compare_view;
mod debug_hud;
mod dock_fit;
mod gh_account;
mod local_shell;
pub(crate) use debug_hud::ToggleDebugOverlay;
mod details_panel;
mod device_agent;
mod devices;
mod diff_comments;
mod env_linked;
mod env_mode;
mod env_picker;
mod env_sidebar;
mod files;
mod focus_areas;
pub(crate) mod folder_watch;
mod gh_pages;
mod gh_pages_render;
mod html_card;
mod image_viewer;
mod import_sessions;
mod keymap_notice;
mod keys;
pub(crate) mod layout;
mod layout_close;
mod left_rail;
pub(crate) mod login_status;
#[cfg(all(test, unix))] // the fake claude is a shell script
mod login_status_test;
pub(crate) mod machines_page;
mod manage;
mod manage_projects;
mod mcp_sign_in;
mod mermaid;
mod mermaid_block;
pub(crate) mod onboarding;
mod settings_tab;
mod setup_flow;
mod side_chat;
mod terminal_close;
mod terminal_launch;
mod terminal_panel;
mod terminal_restore;
mod terminal_split;
mod terminal_tab;
mod terminals;
pub(crate) mod usage_page;
mod usage_page_render;
mod usage_poll;
use terminal_launch::{terminal_env, unbound_root};
mod accounts_file;
mod add_project;
mod delegate_remote;
mod delegated_label;
mod delegation_actions;
pub(crate) mod github;
mod machines_delegation;
mod machines_waiting;
pub(crate) mod orchestrate;
mod orchestrate_ui;
mod palette_forwards;
mod palette_items;
mod panels;
mod projects;
mod question_drafts;
pub(crate) mod quit_guard;
mod remote;
mod remote_answers;
mod remote_queue;
mod remote_reply;
mod remote_thread;
mod render;
mod reopen;
pub(crate) mod resume_after_restart;
pub(crate) mod scheduled;
mod scratch;
mod scripts;
mod status_bar;
pub(crate) mod thread;
use accounts_file::account_label;
pub use accounts_file::{AccountsFile, accounts_now};
mod ask_approval;
mod ask_cards;
mod ask_detail;
mod ask_dock;
mod ask_files;
mod issue_actions;
mod pr_actions;
mod pr_stack;
mod project_rules;
mod project_rules_page;
pub use project_rules_page::ProjectRulesPage;
mod pr_watch;
mod shell_card;
mod smooth_scroll;
mod surfaces;
pub(crate) mod thread_checkpoints;
mod thread_details;
pub(crate) mod thread_extras;
mod thread_items;
mod thread_list;
mod thread_search;
mod timeline_list;
use thread_list::{NewThreadIn, Previews, ToggleProjectScope};
mod cached_workspace;
mod click;
mod diff_view;
mod diff_viewer;
mod diff_viewer_select;
#[cfg(test)]
mod dock_restore_test;
mod find_marks;
mod limit_banner;
mod saved_views;
mod subagent;
mod tab_context_menu;
mod table_copy;
mod thread_failure;
mod thread_find;
mod thread_hooks;
mod thread_model;
mod thread_render;
mod thread_rows;
mod thread_worktree;
mod timeline_fold;
mod timeline_menu;
mod timeline_text;
mod timeline_view;
mod tool_cards;
mod turn_files;
mod update_notice;
#[cfg(test)]
mod update_notice_test;
mod windows;
pub(crate) use click::dispatch;
pub use windows::{
    CloseWindow, NewWindow, WindowKit, open as open_window, restore_windows, set_hub,
    window_handles,
};
mod worktree;

use crate::settings::{Settings, SettingsEvent};
use crate::ui::ActiveTheme as _;
use crate::ui::dock::{DockArea, DockLayout, DockPlacement, panel_handle};
use crate::ui::v_flex;
use center::{Center, Page};
pub use github::{AntigravityBin, CodexBin, CursorBin, GhBin};
pub(crate) use github::{gh_bin, gh_login_options};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, SharedString, Subscription, Task, WeakEntity,
    Window, div, prelude::*, px,
};
use panels::{BrowserPanel, Sidebar, TerminalsEmpty, Welcome};
use skyelab_core::AccountsConfig;
use skyelab_store::{Project, Store, Thread};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use terminal_panel::TerminalPanel;
use thread::ThreadView;

mod actions;
pub use actions::*;

/// The store is sync and not `Sync`; every use goes through a background task holding this lock.
pub type SharedStore = Arc<Mutex<Store>>;

/// The backend this machine's daemon serves, set once at launch; [`Workspace`]'s local source calls it.
pub struct LocalBackend(pub Arc<dyn skyelab_daemon::Backend>);

impl gpui::Global for LocalBackend {}

pub(super) fn lock<T>(m: &Mutex<T>) -> anyhow::Result<std::sync::MutexGuard<'_, T>> {
    m.lock()
        .map_err(|_| anyhow::anyhow!("a background task panicked holding the lock"))
}

/// What threads need to run: where they persist and which binary they launch.
#[derive(Clone)]
pub struct Backend {
    pub store: SharedStore,
    /// `claude`, or a fake in tests and debug builds (`SKYELAB_CLAUDE_BIN`).
    pub claude_bin: PathBuf,
}

const CONTEXT: &str = "Workspace";

pub use keys::init;
pub(crate) use keys::*;

/// Thread id → the matched line of its message (none for a title match).
type SearchHits = std::collections::HashMap<i64, Option<String>>;

pub struct Workspace {
    /// Debug mode's overlay, while shown (docs/debugging.md).
    debug_hud: Option<Entity<debug_hud::DebugHud>>,
    /// Debug mode's live count of windows.
    _live: skyelab_core::debug::Live,
    focus: FocusHandle,
    /// This window, as the window registry (`windows`) knows it.
    window_handle: gpui::AnyWindowHandle,
    dock: Entity<DockArea>,
    /// The dock area's skin: a toggle tells it a dock starts to close (`center_skin.rs`).
    dock_skin: std::rc::Rc<center_skin::CenterSkin>,
    /// The folder the workspace is "in": the selected project, or home before one is picked.
    folder: PathBuf,
    folder_label: SharedString,
    accounts_path: Option<PathBuf>,
    config: AccountsConfig,
    load_error: Option<String>,
    account: Option<SharedString>,
    /// The id of `account`, for its color.
    account_id: Option<String>,
    /// Each project's account (`None`: unbound), resolved off the UI thread since resolving touches the disk.
    project_accounts: std::collections::HashMap<i64, Option<skyelab_core::Account>>,
    _project_accounts: Option<Task<()>>,
    terminals_empty: Option<Entity<TerminalsEmpty>>,
    /// The shown thread's details card, floating over the chat while `details_open` (T3's thread details).
    details: Entity<details_panel::DetailsPanel>,
    details_open: bool,
    /// Asks the accounts' CLIs for their limits while Usage or the details card is shown.
    usage_poll: usage_poll::UsagePoll,
    /// The right dock's first tab, "Open a surface".
    surfaces: Entity<surfaces::SurfacesPanel>,
    layout: layout::LayoutStore,
    _window_bounds: Option<Subscription>,
    _dock_events: Subscription,
    _details_events: Subscription,
    _details_obs: Subscription,
    terminals: Vec<Entity<TerminalPanel>>,
    terms: terminal_close::Terms,
    tab_menu: tab_context_menu::TabContextMenu,
    browsers: Vec<WeakEntity<BrowserPanel>>,
    /// Tests' fake page engine for agent tabs (see `browser_tabs`).
    #[cfg(test)]
    browser_engine: Option<browser_tabs::FakeEngineFactory>,
    /// Created on first open, then kept, so its form and section survive switching pages.
    settings: Option<Entity<Settings>>,
    pub(crate) settings_open: bool,
    backend: Backend,
    /// This machine as an environment: the sidebar's projects and threads are read through it.
    local: Arc<skyelab_env::LocalSource>,
    pub(crate) store_error: Option<SharedString>,
    /// Where scratch threads' per-account folders go (`<data dir>/skyelab/scratch`).
    scratch_root: Option<PathBuf>,
    sidebar: Entity<Sidebar>,
    projects: Vec<Project>,
    project: Option<Project>,
    /// Every listed project's threads, most recently updated first.
    threads: Vec<Thread>,
    previews: Previews,
    /// The project's scripts, for the palette.
    scripts: Vec<skyelab_store::ProjectScript>,
    _scripts: Option<Task<()>>,
    /// The projects the sidebar's list is narrowed to; empty means all.
    scope: Vec<i64>,
    /// Every thread view of this window, shown or not: a hidden one keeps its agent running.
    // shortcut: views are only dropped by close_thread, so a long session keeps every thread it opened in memory.
    open_threads: Vec<Entity<ThreadView>>,
    center: Entity<Center>,
    welcome: Entity<Welcome>,
    bots: Option<Entity<bots_page::BotsPage>>,
    page: Page,
    last_home: Option<Page>,
    setup: setup_flow::SetupFlow,
    back: Vec<Page>,
    forward: Vec<Page>,
    changes_views: Vec<WeakEntity<changes::ChangesView>>,
    /// Pull request diff tabs (`changes_pr.rs`): kept apart, since they read GitHub, not the folder.
    pr_diffs: Vec<WeakEntity<changes::ChangesView>>,
    /// Side chat panes, kept when closed (their agents keep running).
    side_chats: Vec<Entity<side_chat::SideChatPanel>>,
    /// Each open thread view's subscriptions and the row state the sidebar last drew; dropped in `close_thread`.
    thread_subs: std::collections::HashMap<gpui::EntityId, thread_rows::ThreadSubs>,
    /// One-shot waits on a thread view, by key; each removes itself once it fires, or goes when its view closes.
    waiters: std::collections::HashMap<usize, (gpui::EntityId, Subscription)>,
    next_waiter: usize,
    _load: Option<Task<()>>,
    _add: Option<Task<()>>,
    _threads: Option<Task<()>>,
    /// This machine's shell stream (`local_shell.rs`), followed from the first thread load.
    _local_shell: Option<Task<()>>,
    _settings_saved: Option<Subscription>,
    /// Answers delegated starts and sends (see `orchestrate_ui`).
    _orchestrate: Option<Task<()>>,
    _settings_events: Option<Subscription>,
    /// Toasts what is wrong with keybindings.toml each time it loads.
    _keymap: Option<Subscription>,
    devices: Option<WeakEntity<devices::DevicesPanel>>,
    /// Created on first open; tests put a fake here first.
    device_source: Option<std::sync::Arc<dyn devices::DeviceSource>>,
    _appearance: Subscription,
    /// Sharing on the tailnet; `None` until `set_remote` (tests that don't share).
    remote: Option<remote::RemoteHook>,
    /// Which machine the sidebar and remote tabs show (this one unless the picker chose another).
    env: env_mode::EnvMode,
    starts: remote::StartQueue,
    /// Archived threads are listed under their toggle row.
    show_archived: bool,
    /// Which docks (right, bottom) a full-width page (Pull requests, Issues, Bots) put away, to bring back on leaving.
    page_docks: Option<(bool, bool)>,
    compare: compare::State,
    /// A project whose removal waits for the user's OK (it has running agents).
    pending_removal: Option<i64>,
    _dialog_input: Option<Subscription>,
    /// The sidebar's thread filter and, once the store answered, the threads whose title or messages match, each
    /// with its matched line (none for a title match).
    thread_search: Option<(String, Option<SearchHits>)>,
    _search: Option<Task<()>>,
    _attention: Vec<Subscription>,
    pr_watch: pr_watch::Watch,
    gh_pages: gh_pages::Pages,
    /// Deleted threads waiting out their undo window; dropping one (an undo, a new delete) cancels its purge.
    purges: std::collections::HashMap<i64, Task<()>>,
    /// The last unread-mark write; each new one waits for it.
    _unread_write: Option<Task<()>>,
    files: files::FileTabs,
    /// Every notice shown (warnings, undo, thread notifications), for tests: the toast list is private.
    #[cfg(test)]
    notices: Vec<SharedString>,
    /// The last import picker opened, for tests.
    #[cfg(test)]
    import_picker: Option<gpui::WeakEntity<import_sessions::ImportPicker>>,
}

impl Workspace {
    pub fn new(
        folder: PathBuf,
        accounts_path: Option<PathBuf>,
        config: AccountsConfig,
        load_error: Option<String>,
        backend: Backend,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        crate::ui::sync_appearance(window, cx);
        let _appearance = cx.observe_window_appearance(window, |_, window, cx| {
            crate::ui::sync_appearance(window, cx);
        });

        let sidebar = cx.new(|cx| Sidebar::new(Vec::new(), window, cx));
        let welcome = cx.new(Welcome::new);
        let center = cx.new(|cx| Center::new(welcome.clone().into(), cx));
        let terminals_empty = cx.new(TerminalsEmpty::new);
        let details = cx.new(details_panel::DetailsPanel::new);
        let surfaces = cx.new(surfaces::SurfacesPanel::new);

        // Our dock skin, the center drawn uncached (`center_skin.rs`).
        let mut skins = None;
        let workspace = cx.weak_entity();
        let dock = cx.new(|cx| {
            let s = crate::ui::dock::Skin::new(cx);
            let ours = std::rc::Rc::new(center_skin::CenterSkin::new(
                s.clone(),
                center.entity_id(),
                workspace.clone(),
            ));
            skins = Some(ours.clone());
            DockArea::new("workspace", window, cx).with_renderer(ours)
        });
        let dock_skin = skins.expect("the dock area's constructor ran");
        dock.update(cx, |dock, cx| {
            let tabs = |panel, cx: &App| DockLayout::tabs().panel_view(panel, cx);
            dock.set_center(tabs(panel_handle(center.clone()), cx), window, cx);
            dock.set_dock(
                DockPlacement::Left,
                tabs(panel_handle(sidebar.clone()), cx),
                window,
                cx,
            );
            dock.set_dock_size(DockPlacement::Left, px(256.), window, cx);
            dock.set_dock(
                DockPlacement::Bottom,
                tabs(panel_handle(terminals_empty.clone()), cx),
                window,
                cx,
            );
            dock.set_dock_size(DockPlacement::Bottom, px(280.), window, cx);
            dock.toggle_dock(DockPlacement::Bottom, window, cx);
            dock.set_dock(
                DockPlacement::Right,
                tabs(panel_handle(surfaces.clone()), cx),
                window,
                cx,
            );
            dock.set_dock_size(DockPlacement::Right, layout::RIGHT_DEFAULT_W, window, cx);
            dock.toggle_dock(DockPlacement::Right, window, cx);
            // A tab dropped on the center would bring back a tab strip there; nothing is rearranged by dragging.
            dock.set_locked(true, window, cx);
        });
        let _dock_events = cx.subscribe_in(&dock, window, |this, _, e, window, cx| {
            this.dock_changed(e, window, cx)
        });
        let _details_events = cx.subscribe_in(
            &details,
            window,
            |this, _, e: &details_panel::OutsidePress, window, cx| {
                this.details_outside(e.0, window, cx)
            },
        );
        let _details_obs = cx.observe(&details, |_, _, cx| cx.notify());

        crate::ui::set_accounts(cx, &config);
        // Tests and windows without a launch-time daemon backend read their own store with no agents behind it.
        let served = cx.try_global::<LocalBackend>().map_or_else(
            || -> Arc<dyn skyelab_daemon::Backend> {
                Arc::new(skyelab_daemon::StoreBackend::shared(
                    backend.store.clone(),
                    skyelab_daemon::NotWired,
                    accounts_path.clone(),
                ))
            },
            |b| b.0.clone(),
        );
        let local = Arc::new(skyelab_env::LocalSource::new(served));
        let mut this = Self {
            _live: skyelab_core::debug::Live::new("windows"),
            debug_hud: crate::debug_mode::take_open_at_start()
                .then(|| cx.new(|cx| debug_hud::DebugHud::new(false, cx))),
            focus: cx.focus_handle(),
            window_handle: window.window_handle(),
            dock,
            dock_skin,
            account: account_label(&config, &folder),
            account_id: config.resolve(&folder).map(|a| a.id.clone()),
            project_accounts: Default::default(),
            _project_accounts: None,
            folder_label: match dirs::home_dir() {
                Some(home) if folder == home => "~".into(),
                _ => folder.display().to_string().into(),
            },
            folder,
            accounts_path,
            config,
            load_error,
            terminals_empty: Some(terminals_empty),
            details,
            details_open: false,
            usage_poll: Default::default(),
            surfaces,
            layout: Default::default(),
            _window_bounds: None,
            _dock_events,
            _details_events,
            _details_obs,
            terminals: Vec::new(),
            terms: Default::default(),
            tab_menu: Default::default(),
            browsers: Vec::new(),
            #[cfg(test)]
            browser_engine: None,
            settings: None,
            settings_open: false,
            backend,
            local,
            store_error: None,
            scratch_root: dirs::data_dir()
                .map(|d| d.join(skyelab_core::instance::APP_DIR).join("scratch")),
            sidebar,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            previews: Previews::default(),
            scripts: Vec::new(),
            _scripts: None,
            scope: Vec::new(),
            open_threads: Vec::new(),
            center,
            welcome,
            bots: None,
            page: Page::Welcome,
            last_home: None,
            setup: Default::default(),
            back: Vec::new(),
            forward: Vec::new(),
            changes_views: Vec::new(),
            pr_diffs: Vec::new(),
            side_chats: Vec::new(),
            thread_subs: Default::default(),
            waiters: Default::default(),
            next_waiter: 0,
            _load: None,
            _add: None,
            _threads: None,
            _local_shell: None,
            _settings_saved: None,
            _orchestrate: None,
            _settings_events: None,
            _keymap: None,
            devices: None,
            device_source: None,
            _appearance,
            remote: None,
            env: Default::default(),
            starts: remote::StartQueue::new(),
            show_archived: false,
            page_docks: None,
            compare: Default::default(),
            pending_removal: None,
            _dialog_input: None,
            thread_search: None,
            _search: None,
            _attention: Vec::new(),
            pr_watch: Default::default(),
            gh_pages: Default::default(),
            purges: Default::default(),
            _unread_write: None,
            files: Default::default(),
            #[cfg(test)]
            notices: Vec::new(),
            #[cfg(test)]
            import_picker: None,
        };
        this.watch_attention(window, cx);
        this.watch_remote(cx);
        this.watch_setup(cx);
        this.watch_keymap(window, cx);
        this.refresh_details(cx);
        this.refresh_sidebar(cx);
        this.sync_toggles(cx);
        this.load_projects(None, cx);
        this.register_window(window, cx);
        this.resume_recovered_on_start(window, cx);
        this
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

include!("test_mods.rs");
