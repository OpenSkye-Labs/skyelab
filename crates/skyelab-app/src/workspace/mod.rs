//! The main window: sidebar, center tabs, bottom terminals and a status bar, on gpui-component's dock.

mod attention;
#[cfg(unix)]
mod browser_agent;
mod browser_tabs;
mod changes;
mod changes_model;
mod changes_publish;
mod child_slots;
mod devices;
mod env_mode;
mod env_picker;
pub(crate) mod layout;
mod manage;
mod manage_projects;
mod settings_tab;
mod terminal_launch;
mod terminals;
use terminal_launch::{terminal_env, unbound_root};
pub(crate) mod github;
#[cfg(unix)]
pub(crate) mod orchestrate;
#[cfg(not(unix))]
#[path = "orchestrate_stub.rs"]
pub(crate) mod orchestrate;
#[cfg(unix)]
mod orchestrate_ui;
mod palette_items;
mod panels;
mod projects;
mod remote;
mod remote_queue;
mod remote_thread;
mod status_bar;
mod thread;
pub use thread::AccountsFile;
mod ask_cards;
mod shell_card;
mod thread_items;
mod thread_list;
mod thread_model;
mod thread_render;
mod thread_worktree;
mod timeline_text;
mod tool_cards;
mod worktree;

use crate::settings::{Settings, SettingsEvent};
pub use github::{CodexBin, GhBin};
pub(crate) use github::{gh_bin, gh_login_options};
use gpui::{
    Action, App, Context, Entity, FocusHandle, Focusable, KeyBinding, NoAction, SharedString,
    Subscription, Task, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _,
    dock::{DockArea, DockLayout, DockPlacement, DockSkin, panel_handle},
    v_flex,
};
use panels::{BrowserPanel, RightEmpty, Sidebar, TerminalPanel, TerminalsEmpty, Welcome};
use skyelab_core::AccountsConfig;
use skyelab_store::{Project, Store, Thread};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
        OpenDevices
    ]
);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct SelectProject {
    pub id: i64,
}

#[derive(Clone, PartialEq, Action)]
#[action(namespace = skyelab, no_json)]
pub struct OpenThread {
    pub id: i64,
}

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

// `KeyBinding::new` can't resolve `secondary-`, so pick per OS.
const MAC: bool = cfg!(target_os = "macos");
pub(crate) const TOGGLE_SIDEBAR_KEY: &str = if MAC { "cmd-b" } else { "ctrl-b" };
pub(crate) const TOGGLE_BOTTOM_KEY: &str = if MAC { "cmd-j" } else { "ctrl-j" };
pub(crate) const NEW_TERMINAL_KEY: &str = if MAC { "cmd-alt-t" } else { "ctrl-alt-t" };
pub(crate) const OPEN_SETTINGS_KEY: &str = if MAC { "cmd-," } else { "ctrl-," };
pub(crate) const OPEN_BROWSER_KEY: &str = if MAC { "cmd-alt-b" } else { "ctrl-alt-b" };
pub(crate) const OPEN_DEVICES_KEY: &str = if MAC { "cmd-shift-d" } else { "ctrl-alt-d" };

/// Binds the workspace keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(TOGGLE_SIDEBAR_KEY, ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(TOGGLE_BOTTOM_KEY, ToggleBottomPanel, Some(CONTEXT)),
        KeyBinding::new(NEW_TERMINAL_KEY, NewTerminal, Some(CONTEXT)),
        KeyBinding::new(OPEN_SETTINGS_KEY, OpenSettings, Some(CONTEXT)),
        KeyBinding::new(OPEN_BROWSER_KEY, OpenBrowser, Some(CONTEXT)),
        KeyBinding::new(OPEN_DEVICES_KEY, OpenDevices, Some(CONTEXT)),
    ]);
    // Bindings run before a view sees the key, so off macOS ctrl-b/ctrl-j would never reach the shell.
    if !MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_SIDEBAR_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(TOGGLE_BOTTOM_KEY, NoAction, Some("Terminal")),
        ]);
    }
    layout::bind_keys(cx);
    attention::init(cx);
    crate::palette::init(cx);
}

/// The label of the account `folder` is bound to. `None` means unbound: the UI says so, never guesses.
fn account_label(config: &AccountsConfig, folder: &Path) -> Option<SharedString> {
    config.resolve(folder).map(|a| a.label.clone().into())
}

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
    right_empty: Option<Entity<RightEmpty>>,
    layout: layout::LayoutStore,
    _window_bounds: Option<Subscription>,
    _dock_events: Subscription,
    terminals: Vec<Entity<TerminalPanel>>,
    browsers: Vec<WeakEntity<BrowserPanel>>,
    /// Tests' fake page engine for agent tabs (see `browser_tabs`).
    #[cfg(all(test, unix))]
    browser_engine: Option<browser_tabs::FakeEngineFactory>,
    settings: Option<WeakEntity<Settings>>,
    backend: Backend,
    pub(crate) store_error: Option<SharedString>,
    sidebar: Entity<Sidebar>,
    projects: Vec<Project>,
    project: Option<Project>,
    /// The selected project's threads, most recently updated first.
    threads: Vec<Thread>,
    open_threads: Vec<WeakEntity<ThreadView>>,
    /// Local tabs that opened on their own while another machine was shown; added when the user is back here.
    queued_tabs: Vec<Entity<ThreadView>>,
    /// Delegated starts by request id: a retry gets the thread the first call started.
    #[cfg(unix)]
    delegated: std::collections::HashMap<orchestrate_ui::StartKey, orchestrate_ui::DelegatedStart>,
    changes_views: Vec<WeakEntity<changes::ChangesView>>,
    _thread_events: Vec<Subscription>,
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
    /// Every notice shown (warnings, undo notices, thread notifications), for tests: the toast list is private.
    #[cfg(test)]
    notices: Vec<SharedString>,
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
        let terminals_empty = cx.new(TerminalsEmpty::new);
        let right_empty = cx.new(RightEmpty::new);

        let (dock, skin) = DockSkin::dock_area("workspace", None, window, cx);
        skin.set_toggle_button_visible(false, cx);
        dock.update(cx, |dock, cx| {
            let tabs = |panel, cx: &App| DockLayout::tabs().panel_view(panel, cx);
            dock.set_center(tabs(panel_handle(welcome), cx), window, cx);
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
                tabs(panel_handle(right_empty.clone()), cx),
                window,
                cx,
            );
            dock.set_dock_size(DockPlacement::Right, layout::RIGHT_DEFAULT_W, window, cx);
            dock.toggle_dock(DockPlacement::Right, window, cx);
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
            right_empty: Some(right_empty),
            layout: Default::default(),
            _window_bounds: None,
            _dock_events,
            terminals: Vec::new(),
            browsers: Vec::new(),
            #[cfg(all(test, unix))]
            browser_engine: None,
            settings: None,
            backend,
            store_error: None,
            sidebar,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            open_threads: Vec::new(),
            queued_tabs: Vec::new(),
            #[cfg(unix)]
            delegated: Default::default(),
            changes_views: Vec::new(),
            _thread_events: Vec::new(),
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
            #[cfg(test)]
            notices: Vec::new(),
        };
        this.watch_attention(window, cx);
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

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // macOS: with the sidebar hidden, nothing holds room for the traffic lights, so give them a row of their own.
        let lights_row =
            cfg!(target_os = "macos") && !self.dock.read(cx).is_dock_open(DockPlacement::Left);
        let theme = cx.theme();
        v_flex()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_bottom))
            .on_action(cx.listener(Self::toggle_right))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::open_browser))
            .on_action(cx.listener(Self::pick_project))
            .on_action(cx.listener(Self::select_project))
            .on_action(cx.listener(Self::new_thread))
            .on_action(cx.listener(Self::open_thread))
            .on_action(cx.listener(Self::open_devices))
            .on_action(cx.listener(Self::open_remote))
            .on_action(cx.listener(Self::toggle_sharing))
            .on_action(cx.listener(Self::disconnect_peer))
            .on_action(cx.listener(Self::allow_device))
            .on_action(cx.listener(Self::deny_device))
            .on_action(cx.listener(Self::revoke_device))
            .on_action(cx.listener(Self::open_palette))
            .map(|el| self.env_actions(el, cx))
            .map(|el| self.manage_actions(el, cx))
            .map(|el| self.attention_actions(el, cx))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            // Windows: the titlebar is transparent, so draw the window controls in theme colors.
            .when(lights_row, |el| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .h(crate::ui::TOP_ROW_H)
                        .bg(theme.tab_bar),
                )
            })
            .when(cfg!(target_os = "windows"), |el| {
                el.child(gpui_component::TitleBar::new().child("SkyeLab"))
            })
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(self.render_status(cx))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod manage_harness;
#[cfg(test)]
mod manage_projects_test;
#[cfg(test)]
mod manage_sidebar_test;
#[cfg(test)]
mod manage_threads_test;
#[cfg(test)]
mod palette_search_test;
#[cfg(test)]
mod review_fixes_d_test;
#[cfg(test)]
mod thread_keys_test;
#[cfg(all(test, unix))] // the fake agent is a shell script
mod unread_test;

#[cfg(test)]
mod stale_status_test;

#[cfg(test)]
mod browser_tests;

#[cfg(test)]
mod thread_model_test;

#[cfg(test)]
mod timeline_model_test;

#[cfg(all(test, unix))] // builds its thread on the fake agent, a shell script
mod tool_cards_test;

#[cfg(test)]
mod card_path_test;

#[cfg(test)]
mod review_proofs_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod ask_question_e2e_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod delta_store_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod rate_limit_state_test;

#[cfg(test)]
mod threads_test;

#[cfg(test)]
mod sidebar_account_test;

#[cfg(test)]
mod layout_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod sidebar_status_test;

#[cfg(test)]
mod agent_bin_test;

#[cfg(all(test, unix))] // the fake codex is a shell script
mod codex_thread_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod review_second_row_test;

#[cfg(test)]
mod review_stale_config_test;

#[cfg(test)]
mod devices_tests;

#[cfg(test)]
mod devices_review_tests;

#[cfg(test)]
mod changes_model_test;

#[cfg(test)]
mod worktree_thread_test;

#[cfg(test)]
mod worktree_guard_test;

#[cfg(test)]
mod worktree_discard_test;

#[cfg(test)]
mod worktree_review_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod review_cta_folder_test;

#[cfg(test)]
mod env_harness;

#[cfg(test)]
mod palette_test;

#[cfg(all(test, unix))] // the fake gh is a shell script
mod publish_test;

#[cfg(test)]
mod env_switch_test;

#[cfg(test)]
mod env_states_test;

#[cfg(test)]
mod env_picker_test;

#[cfg(test)]
mod env_prompt_loss_test;

#[cfg(test)]
mod env_followups_test;

#[cfg(test)]
mod env_pin_test;

#[cfg(test)]
mod env_whois_test;

#[cfg(test)]
mod env_identity_gate_test;

#[cfg(test)]
mod worktree_followups_test;

#[cfg(test)]
mod remote_start_test;

#[cfg(test)]
mod changes_follow_folder_test;
#[cfg(test)]
mod terminal_link_subdir_test;
#[cfg(test)]
mod terminal_link_test;

#[cfg(test)]
mod delegation_test;

#[cfg(all(test, unix))]
mod delegation_idempotent_test;
#[cfg(all(test, unix))]
mod delegation_reply_on_row_test;
#[cfg(all(test, unix))]
mod delegation_review_test;
#[cfg(all(test, unix))]
mod delegation_send_cap_test;
#[cfg(all(test, unix))]
mod delegation_starting_cap_test;
#[cfg(all(test, unix))]
mod delegation_stopped_parent_test;
#[cfg(all(test, unix))]
mod launch_rebind_test;
#[cfg(all(test, unix))]
mod thread_close_starting_test;

#[cfg(test)]
mod delegation_cap_test;
#[cfg(test)]
mod delegation_resume_test;
#[cfg(test)]
mod delegation_turns_test;
#[cfg(test)]
mod env_approval_test;
#[cfg(test)]
mod env_queue_test;
#[cfg(test)]
mod palette_remote_test;
#[cfg(test)]
mod remote_resume_reply_test;

#[cfg(test)]
mod long_thread_test;

#[cfg(test)]
mod changes_long_test;

#[cfg(all(test, unix))]
mod browser_agent_test;

#[cfg(all(test, unix))]
mod browser_agent_cap_test;

#[cfg(all(test, unix))]
mod browser_agent_review_test;

#[cfg(test)]
mod terminal_env_test;

#[cfg(test)]
mod commit_snapshot_test;

#[cfg(test)]
mod changes_cap_test;
#[cfg(test)]
mod close_tab_test;
#[cfg(test)]
mod drive_rebound_test;
#[cfg(test)]
mod remote_resume_test;
#[cfg(test)]
mod save_retry_stop_test;
#[cfg(test)]
mod save_retry_test;
#[cfg(test)]
mod save_turns_test;
#[cfg(test)]
mod stale_idle_test;

#[cfg(all(test, unix))]
mod accounts_file_reread_test;
#[cfg(all(test, unix))]
mod claude_folded_turns_test;
#[cfg(all(test, unix))]
mod codex_open_failure_turns_test;
#[cfg(all(test, unix))]
mod commit_hook_env_test;
#[cfg(all(test, unix))]
mod delegation_after_stop_test;
#[cfg(all(test, unix))]
mod exit_holding_stdout_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_thread_test;

#[cfg(all(test, unix))] // the fake agent is a shell script
mod permission_mode_test;
