//! Opening terminals in the bottom dock, each under its folder's account: new tabs, split panes, the tabs that
//! follow their thread, and a selection sent to the thread's composer.

use super::terminal_panel::{Pane, SplitTerminal, SplitTerminalDown, TabEvent, TerminalPanel};
use super::{NewTerminal, Workspace};
use gpui::{AppContext as _, Context, Entity, Focusable as _, SharedString, Window};
use gpui_component::dock::{DockPlacement, panel_handle};
use skyelab_core::terminal_layout::Axis;
use skyelab_terminal::{SpawnOptions, Terminal, TerminalEvent, TerminalView};

impl Workspace {
    pub(super) fn new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // the project is on another machine; a local shell here would be misleading
        }
        let job = self.terminal_job(cx);
        // A terminal for the folder the window showed when asked; a switch meanwhile drops it.
        let same_folder = |this: &Self, o: &SpawnOptions| o.cwd == this.folder;
        self.open_terminal(job, same_folder, window, cx, |_, e, _| {
            eprintln!("failed to open terminal: {e:#}")
        });
    }

    pub(super) fn split_terminal(
        &mut self,
        _: &SplitTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused(Axis::Row, window, cx);
    }

    pub(super) fn split_terminal_down(
        &mut self,
        _: &SplitTerminalDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_focused(Axis::Column, window, cx);
    }

    /// Splits the focused pane: a new shell beside it (`Row`) or under it (`Column`), built like a new tab (its
    /// folder's account env).
    fn split_focused(&mut self, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_remote() {
            return;
        }
        let Some((panel, cwd)) = self.terminals.iter().find_map(|p| {
            let at = p.read(cx).focused_pane(window, cx)?;
            let cwd = p.read(cx).panes[at]
                .view
                .read(cx)
                .terminal()
                .cwd()
                .to_path_buf();
            Some((p.clone(), cwd))
        }) else {
            return;
        };
        // The new pane starts where the focused one did (a worktree, a script's folder), under that folder's account.
        let job = self.terminal_job_in(cwd, cx);
        let options = cx.background_spawn(async move { job() });
        cx.spawn_in(window, async move |this, cx| {
            let options = options.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let opened = options.and_then(|(o, account)| {
                    if this.is_remote() || !this.terminals.contains(&panel) {
                        return Ok(()); // the window moved on, or the tab closed, meanwhile
                    }
                    let p = panel.read(cx);
                    let title = format!("{} · {}", p.tab_label(), p.panes.len() + 1).into();
                    let view = this.terminal_view(o, title, window, cx)?;
                    let pane = Pane::new(view, account, cx);
                    panel.update(cx, |p, cx| p.split(pane, axis, window, cx));
                    Ok(())
                });
                if let Err(e) = opened {
                    eprintln!("failed to split terminal: {e:#}");
                }
            });
        })
        .detach();
    }

    /// A tab running one login command, titled after the account and left out of the tabs saved for a restart.
    pub(super) fn add_login_terminal(
        &mut self,
        options: SpawnOptions,
        account: Option<skyelab_core::Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Entity<TerminalPanel>> {
        let title = match &account {
            Some(a) if !a.label.is_empty() => format!("Log in: {}", a.label),
            Some(a) => format!("Log in: {}", a.id),
            None => "Log in".to_owned(),
        };
        let tab = self.add_terminal_tab(options, account.clone(), None, window, cx)?;
        if let Some(account) = account {
            self.recheck_after_login(&tab, account, cx);
        }
        tab.update(cx, |t, cx| {
            t.name = Some(title.into());
            t.one_shot = true;
            cx.notify();
        });
        Ok(tab)
    }

    /// A new tab from `options`; `account` is the one the shell's env was set up for, shown on its tab (`None`:
    /// unbound). `tab` gives its title and text typed into the new shell (a project script).
    pub(super) fn add_terminal_tab(
        &mut self,
        options: SpawnOptions,
        account: Option<skyelab_core::Account>,
        tab: TerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Entity<TerminalPanel>> {
        let (name, input) = match tab {
            Some((title, input)) => (Some(title), Some(input)),
            None => (None, None),
        };
        let label = name
            .clone()
            .unwrap_or_else(|| format!("Terminal {}", self.terminals.len() + 1).into());
        let view = self.terminal_view(options, label, window, cx)?;
        if let Some(input) = input {
            view.read(cx).terminal().write(input.into_bytes()); // read once the shell starts (scripts_env_test)
        }
        let pane = Pane::new(view.clone(), account, cx);
        let owner = self.current_thread();
        let panel = cx.new(|cx| TerminalPanel::new(pane, name, None, cx));
        self.set_terminal_owner(&panel, owner, cx);
        self.add_terminal_panel(panel.clone(), window, cx);
        view.read(cx).focus_handle(cx).focus(window, cx);
        Ok(panel)
    }

    /// The tab goes with `thread`; `None` shows it with every thread.
    pub(super) fn set_terminal_owner(
        &self,
        panel: &Entity<TerminalPanel>,
        thread: Option<Entity<super::ThreadView>>,
        cx: &mut Context<Self>,
    ) {
        panel.update(cx, |p, _| p.owner = thread.map(|t| t.entity_id()));
    }

    /// Docks a new tab and follows what it tells: changes are saved with the window, its × closes it.
    pub(super) fn add_terminal_panel(
        &mut self,
        panel: Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(
            &panel,
            window,
            |this, panel, e: &TabEvent, window, cx| match e {
                TabEvent::Changed => this.schedule_layout_save(window, cx),
                TabEvent::Close => this.close_terminal_tab(panel, window, cx),
            },
        )
        .detach();
        self.dock_terminal(&panel, window, cx);
        self.terminals.push(panel);
    }

    /// Starts a shell with `options` and listens to it; `label` names it on what it adds to a prompt.
    pub(super) fn terminal_view(
        &mut self,
        options: SpawnOptions,
        label: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Entity<TerminalView>> {
        let (terminal, events) = Terminal::spawn(options)?;
        let view = cx.new(|cx| TerminalView::new(terminal, events, cx));
        cx.subscribe_in(
            &view,
            window,
            move |this, _, event, window, cx| match event {
                TerminalEvent::OpenPath(path) => this.reveal_in_changes(path, window, cx),
                TerminalEvent::AddToPrompt(text) => this.add_to_prompt(&label, text, window, cx),
            },
        )
        .detach();
        Ok(view)
    }

    /// Puts `panel` in the bottom dock and opens the dock; the placeholder leaves once a terminal is there.
    fn dock_terminal(
        &mut self,
        panel: &Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = self.terminals_empty.take();
        panel.update(cx, |p, _| p.docked = true);
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
    }

    /// Shows the terminal tabs that go with the thread now shown (its own and those opened with no thread) and takes
    /// the others out of the dock. They keep running, scrollback and all, until their thread shows again.
    pub(super) fn sync_terminals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_saved_terminals(window, cx);
        // Settings and Welcome leave thread terminals as they were.
        let shown = self.current_thread().map(|t| t.entity_id());
        let wanted = |owner: Option<gpui::EntityId>| match (owner, shown) {
            (None, _) => Some(true),
            (Some(o), Some(s)) => Some(o == s),
            (Some(_), None) => None,
        };
        let open = self.dock.read(cx).is_dock_open(DockPlacement::Bottom);
        let (mut show, mut hide) = (Vec::new(), Vec::new());
        for panel in &self.terminals {
            let p = panel.read(cx);
            match (wanted(p.owner), p.docked) {
                (Some(true), false) => show.push(panel.clone()),
                (Some(false), true) => hide.push(panel.clone()),
                _ => {}
            }
        }
        let others_left = self.terminals.iter().any(|p| {
            let p = p.read(cx);
            p.docked && wanted(p.owner) != Some(false)
        });
        if hide.is_empty() && show.is_empty() {
            return;
        }
        if show.is_empty() && !others_left && self.terminals_empty.is_none() {
            // The placeholder goes in first, so the dock never goes empty.
            let empty = cx.new(super::panels::TerminalsEmpty::new);
            self.dock.update(cx, |dock, cx| {
                dock.add_panel_view(
                    panel_handle(empty.clone()),
                    DockPlacement::Bottom,
                    None,
                    window,
                    cx,
                )
            });
            self.terminals_empty = Some(empty);
        }
        for panel in show {
            self.dock_terminal(&panel, window, cx);
        }
        for panel in hide {
            panel.update(cx, |p, _| p.docked = false);
            self.dock
                .update(cx, |dock, cx| dock.remove_panel(panel, window, cx));
        }
        // Taking tabs out or putting them back never opens or closes the dock by itself.
        self.dock.update(cx, |dock, cx| {
            if dock.is_dock_open(DockPlacement::Bottom) != open {
                dock.toggle_dock(DockPlacement::Bottom, window, cx);
            }
        });
    }

    /// A closed thread's terminals keep running and show with every thread from now on.
    pub(super) fn orphan_terminals(
        &mut self,
        thread: gpui::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for panel in &self.terminals {
            panel.update(cx, |p, _| {
                if p.owner == Some(thread) {
                    p.owner = None;
                }
            });
        }
        self.sync_terminals(window, cx);
    }

    /// A terminal's "Add to prompt": the selection goes into the shown thread's composer as a context chip.
    fn add_to_prompt(
        &mut self,
        label: &SharedString,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread) = self.current_thread() else {
            return self.say(
                "Open a thread to add terminal output to its prompt.",
                window,
                cx,
            );
        };
        thread.update(cx, |t, cx| {
            t.add_terminal_context(label.clone(), text, cx);
            t.focus_prompt(window, cx);
        });
    }

    /// A path link from a terminal: shows the first open Changes tab that lists the file; otherwise nothing.
    fn reveal_in_changes(
        &mut self,
        path: &std::path::Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.changes_views.iter().filter_map(|c| c.upgrade());
        for changes in open.collect::<Vec<_>>() {
            if changes.update(cx, |c, cx| c.reveal(path, cx)) {
                self.show_right_panel(changes.entity_id().into(), window, cx);
                return;
            }
        }
    }
}
/// A terminal tab's title and the text typed into its shell once it starts (a project script); `None` for a plain one.
pub(super) type TerminalTab = Option<(gpui::SharedString, String)>;
