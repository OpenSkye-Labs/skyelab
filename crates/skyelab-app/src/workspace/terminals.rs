//! Opening terminals in the bottom dock, each under its folder's account.

use super::panels::TerminalPanel;
use super::{NewTerminal, Workspace};
use gpui::{AppContext as _, Context, Focusable as _, Window};
use gpui_component::dock::{DockPlacement, panel_handle};
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

    /// `account` is the one the shell's env was set up for, shown on its tab (`None`: unbound).
    pub(super) fn add_terminal_with(
        &mut self,
        options: SpawnOptions,
        account: Option<skyelab_core::Account>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        self.add_terminal_tab(options, account, None, window, cx)
    }

    /// [`Self::add_terminal_with`], with `tab`'s title and its text typed into the new shell (a project script).
    pub(super) fn add_terminal_tab(
        &mut self,
        options: SpawnOptions,
        account: Option<skyelab_core::Account>,
        tab: TerminalTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let (terminal, events) = Terminal::spawn(options)?;
        let title = match tab {
            Some((title, input)) => {
                terminal.write(input.into_bytes()); // read once the shell starts (scripts_env_test)
                title
            }
            None => format!("Terminal {}", self.terminals.len() + 1).into(),
        };
        let view = cx.new(|cx| TerminalView::new(terminal, events, cx));
        cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            TerminalEvent::OpenPath(path) => this.reveal_in_changes(path, window, cx),
        })
        .detach();
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
