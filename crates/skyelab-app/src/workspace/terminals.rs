//! Opening terminals in the bottom dock, each under its folder's account.

use super::panels::TerminalPanel;
use super::{NewTerminal, Workspace, terminal_env, unbound_root};
use gpui::{AppContext as _, Context, Focusable as _, Window};
use gpui_component::dock::{DockPlacement, panel_handle};
use skyelab_terminal::{SpawnOptions, Terminal, TerminalView};

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
        if let Err(e) = self.add_terminal(window, cx) {
            eprintln!("failed to open terminal: {e:#}");
        }
    }

    /// What a new terminal runs with: the current folder, under its bound account or none.
    pub(super) fn terminal_options(&self) -> anyhow::Result<SpawnOptions> {
        Ok(SpawnOptions {
            cwd: self.folder.clone(),
            command: None,
            env: terminal_env(&self.config, &self.folder, &unbound_root()?)?,
            cols: 80,
            lines: 24,
        })
    }

    fn add_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let options = self.terminal_options()?;
        self.add_terminal_with(options, window, cx)
    }

    pub(super) fn add_terminal_with(
        &mut self,
        options: SpawnOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let cwd = options.cwd.clone();
        let (terminal, events) = Terminal::spawn(options)?;
        let view = cx.new(|cx| TerminalView::new(terminal, events, cx));
        let title = format!("Terminal {}", self.terminals.len() + 1).into();
        // The same config terminal_env used to pick the shell's account.
        let account = self.config.resolve(&cwd).cloned();
        let panel = cx.new(|_| TerminalPanel::new(view.clone(), title, account));
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
}
