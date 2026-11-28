//! Terminal tabs across a restart: each window saves its tabs (layout, names, folders) with its frame, and reopens
//! them with new shells. A thread's tabs open the first time that thread shows in the window, so a launch starts
//! no shells for threads nobody looks at. Scrollback isn't kept: it can hold secrets, and the shells are new.

use super::Workspace;
use super::terminal_panel::{Pane, TerminalPanel};
use super::thread::ThreadView;
use crate::ui::dock::DockPlacement;
use gpui::{App, AppContext as _, Context, Entity, Window};
use skyelab_core::terminal_layout::{PaneTree, SavedTerminalTab};
use std::path::PathBuf;

impl Workspace {
    /// The tabs saved with this window: the open ones in tab order, then those still waiting for their thread. A
    /// tab's thread is its owner's store id now (a new thread gets its row only at its first send).
    pub(super) fn terminal_layouts(&self, cx: &App) -> Vec<SavedTerminalTab> {
        let thread = |owner: Option<gpui::EntityId>| {
            let view = self
                .open_threads
                .iter()
                .find(|v| Some(v.entity_id()) == owner)?;
            view.read(cx).thread.as_ref().map(|t| t.id)
        };
        self.terminals
            .iter()
            .filter(|p| !p.read(cx).one_shot)
            .map(|p| SavedTerminalTab {
                thread: thread(p.read(cx).owner),
                ..p.read(cx).saved()
            })
            .chain(self.terms.restoring.iter().map(|(_, t)| t.clone()))
            .chain(self.terms.saved.iter().cloned())
            .collect()
    }

    /// At launch: keeps the window's saved tabs and opens those that go with what shows now.
    pub(super) fn restore_terminals(
        &mut self,
        tabs: Vec<SavedTerminalTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terms.saved = tabs;
        self.open_saved_terminals(window, cx);
    }

    /// Opens the saved tabs that show now: those with no thread, and the shown thread's.
    pub(super) fn open_saved_terminals(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terms.saved.is_empty() || self.is_remote() {
            return;
        }
        let shown = self.current_thread();
        let shown_id = shown
            .as_ref()
            .and_then(|t| t.read(cx).thread.as_ref().map(|t| t.id));
        let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.terms.saved)
            .into_iter()
            .partition(|t| t.thread.is_none() || t.thread == shown_id);
        self.terms.saved = later;
        for tab in now {
            let owner = tab.thread.and(shown.clone());
            self.restore_tab(tab, owner, window, cx);
        }
    }

    /// Starts a shell per saved pane, each through the same job as a new terminal (its folder's account env), and
    /// puts the tab back in its place. A pane whose folder is gone is left out.
    fn restore_tab(
        &mut self,
        tab: SavedTerminalTab,
        owner: Option<Entity<ThreadView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tree) = tab.panes.clone().repaired() else {
            return;
        };
        let key = self.terms.next_restore;
        self.terms.next_restore += 1;
        self.terms.restoring.push((key, tab.clone()));
        let folders: Vec<PathBuf> = tree.leaves().into_iter().cloned().collect();
        let jobs: Vec<_> = folders
            .iter()
            .map(|f| (f.is_dir(), self.terminal_job_in(f.clone(), cx)))
            .collect();
        let options = cx.background_spawn(async move {
            jobs.into_iter()
                .map(|(exists, job)| {
                    anyhow::ensure!(exists, "the folder is gone");
                    job()
                })
                .collect::<Vec<_>>()
        });
        let mut n = 0;
        let mut index = tree.map(&mut |_| {
            n += 1;
            n - 1
        });
        cx.spawn_in(window, async move |this, cx| {
            let options = options.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.terms.restoring.retain(|(k, _)| *k != key);
                if this.is_remote() {
                    this.terms.saved.push(tab); // back to waiting: this window shows another machine now
                    return;
                }
                let mut panes = Vec::new();
                for (i, opened) in options.into_iter().enumerate() {
                    let view = opened.and_then(|(o, account)| {
                        let label = format!("Terminal {}", this.terminals.len() + 1).into();
                        Ok((this.terminal_view(o, label, window, cx)?, account))
                    });
                    match view {
                        Ok((view, account)) => panes.push((i, Pane::new(view, account, cx))),
                        Err(e) => {
                            eprintln!(
                                "couldn't reopen a terminal in {}: {e:#}",
                                folders[i].display()
                            );
                            index = match index.clone().remove(&i) {
                                Some(t) => t,
                                None => return,
                            };
                        }
                    }
                }
                let tree: PaneTree<gpui::EntityId> = index.map(&mut |i| {
                    let (_, p) = panes
                        .iter()
                        .find(|(at, _)| at == i)
                        .expect("every kept leaf opened");
                    p.view.entity_id()
                });
                let panes = panes.into_iter().map(|(_, p)| p).collect();
                let name = tab.name.map(Into::into);
                let panel =
                    cx.new(|cx| TerminalPanel::with_tree(panes, Some(tree), name, None, cx));
                this.set_terminal_owner(&panel, owner, cx);
                // Back in the dock without opening it: the user opens it when they want the terminals.
                let open = this.dock.read(cx).is_dock_open(DockPlacement::Bottom);
                this.add_terminal_panel(panel, window, cx);
                this.dock.update(cx, |dock, cx| {
                    if dock.is_dock_open(DockPlacement::Bottom) != open {
                        dock.toggle_dock(DockPlacement::Bottom, window, cx);
                    }
                });
                this.sync_terminals(window, cx);
            });
        })
        .detach();
    }
}
