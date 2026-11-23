//! The main window: sidebar, the center's one page, bottom terminals and a status bar, on gpui-component's dock.

mod attention;
mod branch_picker;
mod browser_agent;
mod browser_tabs;
mod center;
mod changes;
mod changes_generate;
mod changes_model;
mod changes_open;
mod changes_publish;
mod changes_tree;
mod changes_turns;
mod child_slots;
mod details_panel;
mod devices;
mod env_mode;
mod env_picker;
mod env_sidebar;
mod files;
mod import_sessions;
mod keys;
pub(crate) mod layout;
mod manage;
mod manage_projects;
mod onboarding;
mod settings_tab;
mod setup_flow;
mod terminal_launch;
mod terminal_panel;
mod terminals;
use terminal_launch::{terminal_env, unbound_root};
mod accounts_file;
pub(crate) mod github;
pub(crate) mod orchestrate;
mod orchestrate_ui;
mod palette_items;
mod panels;
mod projects;
mod question_drafts;
mod remote;
mod remote_answers;
mod remote_queue;
mod remote_thread;
mod render;
mod scripts;
mod status_bar;
mod thread;
use accounts_file::account_label;
pub use accounts_file::{AccountsFile, accounts_now};
mod ask_cards;
mod shell_card;
pub(crate) mod thread_checkpoints;
mod thread_details;
pub(crate) mod thread_extras;
mod thread_items;
mod thread_list;
use thread_list::{NewThreadIn, Previews, ToggleProjectScope};
mod subagent;
mod thread_failure;
mod thread_find;
mod thread_model;
mod thread_render;
mod thread_rows;
mod thread_worktree;
mod timeline_fold;
mod timeline_text;
mod tool_cards;
mod turn_files;
mod worktree;

use crate::settings::{Settings, SettingsEvent};
use center::{Center, Page};
pub use github::{CodexBin, GhBin};
pub(crate) use github::{gh_bin, gh_login_options};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable, SharedString, Subscription, Task, WeakEntity,
    Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    dock::{DockArea, DockLayout, DockPlacement, DockSkin, panel_handle},
    v_flex,
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

actions!(
    skyelab,
    [
        ToggleSidebar,
        ToggleBottomPanel,
        NewTerminal,
        OpenSettings,
        OpenBrowser,
        AddProject,
        NewThread,
        OpenDevices,
        ShowAllProjects,
        GoBack,
        GoForward,
        SetUpAccounts
    ]
);

/// The store is sync and not `Sync`; every use goes through a background task holding this lock.
pub type SharedStore = Arc<Mutex<Store>>;

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

pub struct Workspace {
    focus: FocusHandle,
    dock: Entity<DockArea>,
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
    /// The right dock's placeholder until a tool opens there.
    details: Entity<details_panel::DetailsPanel>,
    layout: layout::LayoutStore,
    _window_bounds: Option<Subscription>,
    _dock_events: Subscription,
    terminals: Vec<Entity<TerminalPanel>>,
    browsers: Vec<WeakEntity<BrowserPanel>>,
    /// Tests' fake page engine for agent tabs (see `browser_tabs`).
    #[cfg(test)]
    browser_engine: Option<browser_tabs::FakeEngineFactory>,
    /// Created on first open, then kept, so its form and section survive switching pages.
    settings: Option<Entity<Settings>>,
    backend: Backend,
    pub(crate) store_error: Option<SharedString>,
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
    // shortcut: views are only dropped by close_thread (package D's archive/delete), so a long session keeps every
    // thread it opened in memory; evict idle hidden ones if that ever matters.
    open_threads: Vec<Entity<ThreadView>>,
    center: Entity<Center>,
    welcome: Entity<Welcome>,
    page: Page,
    setup: setup_flow::SetupFlow,
    back: Vec<Page>,
    forward: Vec<Page>,
    /// Delegated starts by request id: a retry gets the thread the first call started.
    delegated: std::collections::HashMap<orchestrate_ui::StartKey, orchestrate_ui::DelegatedStart>,
    changes_views: Vec<WeakEntity<changes::ChangesView>>,
    /// Each open thread view's subscriptions and the row state the sidebar last drew; dropped in `close_thread`.
    thread_subs: std::collections::HashMap<gpui::EntityId, thread_rows::ThreadSubs>,
    /// One-shot waits on a thread view, by key; each removes itself once it fires, or goes when its view closes.
    waiters: std::collections::HashMap<usize, (gpui::EntityId, Subscription)>,
    next_waiter: usize,
    _load: Option<Task<()>>,
    _add: Option<Task<()>>,
    _threads: Option<Task<()>>,
    _settings_saved: Option<Subscription>,
    /// Answers delegated starts and sends (see `orchestrate_ui`).
    _orchestrate: Option<Task<()>>,
    _settings_events: Option<Subscription>,
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
    /// A project whose removal waits for the user's OK (it has running agents).
    pending_removal: Option<i64>,
    _dialog_input: Option<Subscription>,
    /// The sidebar's thread filter and, once the store answered, the threads whose title or messages match.
    thread_search: Option<(String, Option<std::collections::HashSet<i64>>)>,
    _search: Option<Task<()>>,
    _attention: Vec<Subscription>,
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

        let (dock, skin) = DockSkin::dock_area("workspace", None, window, cx);
        skin.set_toggle_button_visible(false, cx);
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
                tabs(panel_handle(details.clone()), cx),
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

        crate::ui::set_accounts(cx, &config);
        let mut this = Self {
            focus: cx.focus_handle(),
            dock,
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
            layout: Default::default(),
            _window_bounds: None,
            _dock_events,
            terminals: Vec::new(),
            browsers: Vec::new(),
            #[cfg(test)]
            browser_engine: None,
            settings: None,
            backend,
            store_error: None,
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
            page: Page::Welcome,
            setup: Default::default(),
            back: Vec::new(),
            forward: Vec::new(),
            delegated: Default::default(),
            changes_views: Vec::new(),
            thread_subs: Default::default(),
            waiters: Default::default(),
            next_waiter: 0,
            _load: None,
            _add: None,
            _threads: None,
            _settings_saved: None,
            _orchestrate: None,
            _settings_events: None,
            devices: None,
            device_source: None,
            _appearance,
            remote: None,
            env: Default::default(),
            starts: remote::StartQueue::new(),
            show_archived: false,
            pending_removal: None,
            _dialog_input: None,
            thread_search: None,
            _search: None,
            _attention: Vec::new(),
            purges: Default::default(),
            _unread_write: None,
            files: Default::default(),
            #[cfg(test)]
            notices: Vec::new(),
            #[cfg(test)]
            import_picker: None,
        };
        this.watch_attention(window, cx);
        this.watch_setup(cx);
        this.refresh_details(cx);
        this.refresh_sidebar(cx);
        this.sync_toggles(cx);
        this.load_projects(None, cx);
        this
    }

    fn toggle(&mut self, at: DockPlacement, window: &mut Window, cx: &mut Context<Self>) {
        self.dock
            .update(cx, |dock, cx| dock.toggle_dock(at, window, cx));
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle(DockPlacement::Left, window, cx);
    }

    fn toggle_bottom(
        &mut self,
        _: &ToggleBottomPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle(DockPlacement::Bottom, window, cx);
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(all(test, unix))]
mod accounts_file_reread_test;
#[cfg(test)]
mod agent_bin_test;
#[cfg(all(test, unix))]
mod browser_agent_cap_test;
#[cfg(all(test, unix))]
mod browser_agent_review_test;
#[cfg(all(test, unix))]
mod browser_agent_test;
#[cfg(test)]
mod browser_tests;
#[cfg(test)]
mod changes_cap_test;
#[cfg(test)]
mod changes_find_test;
#[cfg(test)]
mod changes_follow_folder_test;
#[cfg(test)]
mod changes_long_test;
#[cfg(test)]
mod changes_model_test;
#[cfg(all(test, unix))]
mod changes_selection_test;
#[cfg(test)]
mod changes_tree_test;
#[cfg(test)]
mod changes_whitespace_test;
#[cfg(all(test, unix))]
mod checkpoint_midturn_test;
#[cfg(test)]
mod checkpoint_purge_test;
#[cfg(all(test, unix))]
mod checkpoint_turns_test;
#[cfg(all(test, unix))]
mod claude_folded_turns_test;
#[cfg(test)]
mod close_tab_test;
#[cfg(all(test, unix))]
mod codex_open_failure_turns_test;
#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_thread_test;
#[cfg(all(test, unix))]
mod commit_hook_env_test;
#[cfg(test)]
mod commit_snapshot_test;
#[cfg(all(test, unix))]
mod delegation_after_stop_test;
#[cfg(test)]
mod delegation_cap_test;
#[cfg(all(test, unix))]
mod delegation_idempotent_test;
#[cfg(all(test, unix))]
mod delegation_reply_on_row_test;
#[cfg(test)]
mod delegation_resume_test;
#[cfg(all(test, unix))]
mod delegation_review_test;
#[cfg(all(test, unix))]
mod delegation_send_cap_test;
#[cfg(all(test, unix))]
mod delegation_starting_cap_test;
#[cfg(all(test, unix))]
mod delegation_stopped_parent_test;
#[cfg(test)]
mod delegation_test;
#[cfg(test)]
mod delegation_turns_test;
#[cfg(test)]
mod devices_review_tests;
#[cfg(test)]
mod devices_tests;
#[cfg(test)]
mod diff_prefs_test;
#[cfg(test)]
mod drive_rebound_test;
#[cfg(test)]
mod env_approval_test;
#[cfg(test)]
mod env_flat_list_test;
#[cfg(test)]
mod env_followups_test;
#[cfg(test)]
mod env_harness;
#[cfg(test)]
mod env_identity_gate_test;
#[cfg(test)]
mod env_picker_test;
#[cfg(test)]
mod env_pin_test;
#[cfg(test)]
mod env_prompt_loss_test;
#[cfg(test)]
mod env_queue_test;
#[cfg(test)]
mod env_states_test;
#[cfg(test)]
mod env_switch_test;
#[cfg(test)]
mod env_whois_test;
#[cfg(all(test, unix))]
mod exit_holding_stdout_test;
#[cfg(all(test, unix))] // uses publish_test's fake-gh helpers
mod generate_text_test;
#[cfg(all(test, unix))]
mod import_sessions_test;
#[cfg(all(test, unix))]
mod launch_rebind_test;
#[cfg(test)]
mod layout_test;
#[cfg(test)]
mod long_thread_test;
#[cfg(test)]
mod manage_harness;
#[cfg(test)]
mod manage_projects_test;
#[cfg(test)]
mod manage_threads_test;
#[cfg(all(test, unix))]
mod onboarding_test;
#[cfg(test)]
mod palette_remote_test;
#[cfg(test)]
mod palette_search_test;
#[cfg(test)]
mod palette_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_mode_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_thread_test;
#[cfg(all(test, unix))]
mod project_unread_restart_test;
#[cfg(all(test, unix))] // the fake gh is a shell script
mod publish_test;
#[cfg(test)]
mod remote_resume_reply_test;
#[cfg(test)]
mod remote_resume_test;
#[cfg(test)]
mod remote_start_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod review_cta_folder_test;
#[cfg(test)]
mod review_fixes_tests;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod review_second_row_test;
#[cfg(test)]
mod review_stale_config_test;
#[cfg(test)]
mod save_retry_stop_test;
#[cfg(test)]
mod save_retry_test;
#[cfg(test)]
mod save_turns_test;
#[cfg(all(test, unix))]
mod saved_view_draft_test;
#[cfg(all(test, unix))]
mod scripts_env_test;
#[cfg(test)]
mod scripts_palette_test;
#[cfg(all(test, unix))]
mod settings_reopen_stale_test;
#[cfg(test)]
mod sidebar_tests;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod single_view_merge_test;
#[cfg(all(test, unix))]
mod single_view_test;
#[cfg(test)]
mod stale_idle_test;
#[cfg(test)]
mod stale_status_test;
#[cfg(test)]
mod terminal_context_test;
#[cfg(test)]
mod terminal_env_test;
#[cfg(test)]
mod terminal_link_subdir_test;
#[cfg(test)]
mod terminal_link_test;
#[cfg(test)]
mod terminal_owner_test;
#[cfg(test)]
mod terminal_split_test;
#[cfg(test)]
mod tests;
#[cfg(all(test, unix))]
mod thread_close_starting_test;
#[cfg(test)]
mod thread_keys_test;
#[cfg(test)]
mod thread_model_test;
#[cfg(test)]
mod threads_test;
#[cfg(test)]
mod timeline_tests;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod unread_test;
#[cfg(all(test, unix))]
mod worktree_base_test;
#[cfg(test)]
mod worktree_discard_test;
#[cfg(test)]
mod worktree_followups_test;
#[cfg(test)]
mod worktree_guard_test;
#[cfg(test)]
mod worktree_review_test;
#[cfg(test)]
mod worktree_thread_test;
