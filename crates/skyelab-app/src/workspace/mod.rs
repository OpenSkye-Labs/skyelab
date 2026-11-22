//! The main window: sidebar, center tabs, bottom terminals and a status bar, on gpui-component's dock.

mod panels;
mod projects;
mod thread;
mod thread_model;
mod thread_render;

use crate::settings::Settings;
use gpui::{
    Action, App, Context, Entity, FocusHandle, Focusable, KeyBinding, NoAction, SharedString,
    Subscription, Task, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Sizable as _, Theme,
    button::{Button, ButtonVariants as _},
    dock::{DockArea, DockLayout, DockPlacement, DockSkin, panel_handle},
    h_flex,
    status_bar::StatusBar,
    v_flex,
};
use panels::{Sidebar, TerminalPanel, TerminalsEmpty, Welcome};
use skyelab_core::AccountsConfig;
use skyelab_store::{Project, Store, Thread};
use skyelab_terminal::{SpawnOptions, Terminal, TerminalView};
use std::collections::HashMap;
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
        AddProject,
        NewThread
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

/// Binds the workspace keys. Call once at startup.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(TOGGLE_SIDEBAR_KEY, ToggleSidebar, Some(CONTEXT)),
        KeyBinding::new(TOGGLE_BOTTOM_KEY, ToggleBottomPanel, Some(CONTEXT)),
        KeyBinding::new(NEW_TERMINAL_KEY, NewTerminal, Some(CONTEXT)),
        KeyBinding::new(OPEN_SETTINGS_KEY, OpenSettings, Some(CONTEXT)),
    ]);
    // Bindings run before a view sees the key, so off macOS ctrl-b/ctrl-j would never reach the shell.
    if !MAC {
        cx.bind_keys([
            KeyBinding::new(TOGGLE_SIDEBAR_KEY, NoAction, Some("Terminal")),
            KeyBinding::new(TOGGLE_BOTTOM_KEY, NoAction, Some("Terminal")),
        ]);
    }
}

/// Env for a terminal in `dir`: the bound account's, or none, so skyelab-terminal clears the
/// inherited `CLAUDE_CONFIG_DIR` and the shell never runs `claude` under the parent's account.
fn terminal_env(config: &AccountsConfig, dir: &Path) -> anyhow::Result<HashMap<String, String>> {
    if config.resolve(dir).is_none() {
        return Ok(HashMap::new());
    }
    config
        .launch_env(dir)?
        .into_iter()
        .map(|(k, v)| match v.to_str() {
            Some(v) => Ok((k.to_owned(), v.to_owned())),
            None => anyhow::bail!("{k} for {} is not valid UTF-8", dir.display()),
        })
        .collect()
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
    terminals_empty: Option<Entity<TerminalsEmpty>>,
    terminals: Vec<Entity<TerminalPanel>>,
    settings: Option<WeakEntity<Settings>>,
    backend: Backend,
    pub(crate) store_error: Option<SharedString>,
    sidebar: Entity<Sidebar>,
    projects: Vec<Project>,
    project: Option<Project>,
    /// The selected project's threads, newest first.
    threads: Vec<Thread>,
    open_threads: Vec<WeakEntity<ThreadView>>,
    _thread_events: Vec<Subscription>,
    _load: Option<Task<()>>,
    _add: Option<Task<()>>,
    _threads: Option<Task<()>>,
    _settings_saved: Option<Subscription>,
    _appearance: Subscription,
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
        Theme::sync_system_appearance(Some(window), cx);
        let _appearance = cx.observe_window_appearance(window, |_, window, cx| {
            Theme::sync_system_appearance(Some(window), cx);
        });

        let sidebar = cx.new(|cx| Sidebar::new(Vec::new(), cx));
        let welcome = cx.new(Welcome::new);
        let terminals_empty = cx.new(TerminalsEmpty::new);

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
            dock.set_dock_size(DockPlacement::Left, px(260.), window, cx);
            dock.set_dock(
                DockPlacement::Bottom,
                tabs(panel_handle(terminals_empty.clone()), cx),
                window,
                cx,
            );
            dock.set_dock_size(DockPlacement::Bottom, px(280.), window, cx);
            dock.toggle_dock(DockPlacement::Bottom, window, cx);
        });

        let mut this = Self {
            focus: cx.focus_handle(),
            dock,
            account: account_label(&config, &folder),
            folder_label: match dirs::home_dir() {
                Some(home) if folder == home => "~".into(),
                _ => folder.display().to_string().into(),
            },
            folder,
            accounts_path,
            config,
            load_error,
            terminals_empty: Some(terminals_empty),
            terminals: Vec::new(),
            settings: None,
            backend,
            store_error: None,
            sidebar,
            projects: Vec::new(),
            project: None,
            threads: Vec::new(),
            open_threads: Vec::new(),
            _thread_events: Vec::new(),
            _load: None,
            _add: None,
            _threads: None,
            _settings_saved: None,
            _appearance,
        };
        this.refresh_sidebar(cx);
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

    fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.add_terminal(window, cx) {
            eprintln!("failed to open terminal: {e:#}");
        }
    }

    fn add_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let (terminal, events) = Terminal::spawn(SpawnOptions {
            cwd: self.folder.clone(),
            command: None,
            env: terminal_env(&self.config, &self.folder)?,
            cols: 80,
            lines: 24,
        })?;
        let view = cx.new(|cx| TerminalView::new(terminal, events, cx));
        let title = format!("Terminal {}", self.terminals.len() + 1).into();
        let panel = cx.new(|_| TerminalPanel::new(view.clone(), title));
        let empty = self.terminals_empty.take();
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(panel.clone()),
                DockPlacement::Bottom,
                None,
                window,
                cx,
            );
            // Added before the placeholder leaves, so the dock never goes empty.
            if let Some(empty) = empty {
                dock.remove_panel(empty, window, cx);
            }
            if !dock.is_dock_open(DockPlacement::Bottom) {
                dock.toggle_dock(DockPlacement::Bottom, window, cx);
            }
        });
        self.terminals.push(panel);
        view.read(cx).focus_handle(cx).focus(window, cx);
        Ok(())
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(settings) = self.settings.as_ref().and_then(|s| s.upgrade()) {
            let id = settings.entity_id().into();
            self.dock
                .update(cx, |dock, cx| dock.select_panel(id, window, cx));
            settings.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        self.reload_accounts();
        let (path, config, error) = (
            self.accounts_path.clone(),
            self.config.clone(),
            self.load_error.clone(),
        );
        let settings = cx.new(|cx| Settings::new(path, config, error, window, cx));
        self.dock.update(cx, |dock, cx| {
            dock.add_panel_view(
                panel_handle(settings.clone()),
                DockPlacement::Center,
                None,
                window,
                cx,
            );
        });
        settings.read(cx).focus_handle(cx).focus(window, cx);
        // Settings keeps its own copy and saves to disk, so re-read the file whenever it changes.
        self._settings_saved = Some(cx.observe(&settings, |this, _, cx| {
            this.reload_accounts();
            this.push_config(cx);
            cx.notify();
        }));
        self.settings = Some(settings.downgrade());
        cx.notify();
    }

    fn reload_accounts(&mut self) {
        let Some(path) = &self.accounts_path else {
            return;
        };
        match AccountsConfig::load(path) {
            Ok(config) => {
                (self.config, self.load_error) = (config, None);
                self.account = account_label(&self.config, &self.folder);
            }
            Err(e) => {
                self.load_error = Some(format!("{e:#}"));
                // A broken file binds nothing we can trust.
                self.account = None;
            }
        }
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (dot, label) = match &self.account {
            Some(label) => (theme.success, label.clone()),
            None => (theme.warning, "No account bound".into()),
        };
        let account = Button::new("status-account")
            .ghost()
            .xsmall()
            .tooltip("Accounts and folder bindings")
            .child(
                h_flex()
                    .gap_1p5()
                    .child(div().size(px(6.)).rounded_full().bg(dot))
                    .child(label),
            )
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx));
        StatusBar::new()
            .left(
                h_flex()
                    .gap_3()
                    .text_xs()
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child(self.folder_label.clone()),
                    )
                    .children(self.store_error.clone().map(|e| {
                        div()
                            .text_color(theme.danger)
                            .child(format!("Store error: {e}"))
                    })),
            )
            .right(account)
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id("workspace")
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_bottom))
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::pick_project))
            .on_action(cx.listener(Self::select_project))
            .on_action(cx.listener(Self::new_thread))
            .on_action(cx.listener(Self::open_thread))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(div().flex_1().min_h_0().child(self.dock.clone()))
            .child(self.render_status(cx))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stale_status_test;

#[cfg(test)]
mod thread_model_test;

#[cfg(test)]
mod threads_test;
