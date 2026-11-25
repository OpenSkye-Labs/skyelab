//! One terminal tab in the bottom dock: panes in a split tree (side by side and stacked, T3's split terminal), each
//! its own shell. Drawing the tree and its dividers is in `terminal_split`, the tab's title and rename in
//! `terminal_tab`.

use gpui::{
    App, AppContext as _, Context, Entity, EntityId, EventEmitter, FocusHandle, Focusable,
    KeyBinding, SharedString, Subscription, Task, Window, actions,
};
use gpui_component::input::InputState;
use skyelab_core::terminal_layout::{Axis, PaneTree, SavedTerminalTab};
use skyelab_terminal::{Foreground, TerminalView};
use std::path::PathBuf;
use std::time::Duration;

actions!(
    skyelab,
    [
        SplitTerminal,
        SplitTerminalDown,
        CloseTerminal,
        FocusNextTerminal,
        FocusPreviousTerminal
    ]
);

const MAC: bool = cfg!(target_os = "macos");
// ctrl-d is end-of-file in a shell, hence ctrl-shift-d off macOS.
pub(crate) const SPLIT_TERMINAL_KEY: &str = if MAC { "cmd-d" } else { "ctrl-shift-d" };
pub(crate) const SPLIT_DOWN_KEY: &str = if MAC {
    "cmd-shift-d"
} else {
    "ctrl-alt-shift-d"
};
// ctrl-w deletes a word in a shell, hence ctrl-shift-w off macOS.
pub(crate) const CLOSE_TERMINAL_KEY: &str = if MAC { "cmd-w" } else { "ctrl-shift-w" };
pub(crate) const NEXT_TERMINAL_KEY: &str = if MAC {
    "cmd-alt-right"
} else {
    "ctrl-alt-right"
};
pub(crate) const PREVIOUS_TERMINAL_KEY: &str = if MAC { "cmd-alt-left" } else { "ctrl-alt-left" };
/// How often a shown tab asks what its shells run (its title) and where (saved for restore).
const POLL: Duration = Duration::from_millis(1500);

/// Binds split, close and pane focus in a terminal. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    let c = Some("Terminal");
    cx.bind_keys([
        KeyBinding::new(SPLIT_TERMINAL_KEY, SplitTerminal, c),
        KeyBinding::new(SPLIT_DOWN_KEY, SplitTerminalDown, c),
        KeyBinding::new(CLOSE_TERMINAL_KEY, CloseTerminal, c),
        KeyBinding::new(NEXT_TERMINAL_KEY, FocusNextTerminal, c),
        KeyBinding::new(PREVIOUS_TERMINAL_KEY, FocusPreviousTerminal, c),
    ]);
}

/// A shell in a tab and the account its env was built for (`None`: unbound folder).
pub struct Pane {
    pub view: Entity<TerminalView>,
    pub account: Option<skyelab_core::Account>,
    /// What the shell ran and where, at the last poll.
    pub now: Foreground,
    /// The last folder the shell was known to be in: where it reopens after a restart.
    pub cwd: PathBuf,
}

impl Pane {
    pub fn new(
        view: Entity<TerminalView>,
        account: Option<skyelab_core::Account>,
        cx: &App,
    ) -> Self {
        let cwd = view.read(cx).terminal().cwd().to_path_buf();
        Self {
            view,
            account,
            now: Foreground::default(),
            cwd,
        }
    }

    pub(super) fn id(&self) -> EntityId {
        self.view.entity_id()
    }
}

/// What a tab tells the workspace.
pub enum TabEvent {
    /// The layout, a name or a folder changed: save the window's layout.
    Changed,
    /// The tab's × was pressed: close the whole tab (after the confirm, if a shell is busy).
    Close,
    /// The tab bar's Split right (`Row`) or Split down (`Column`): split the tab's active pane.
    Split(Axis),
    /// The tab bar's New terminal.
    New,
}

pub struct TerminalPanel {
    /// In reading order (the tree's leaves); never empty.
    pub panes: Vec<Pane>,
    /// How the panes are laid out; its leaves are the panes' view ids.
    pub tree: PaneTree<EntityId>,
    /// A name the user (or a project script) gave the tab; `None` follows the focused pane.
    pub name: Option<SharedString>,
    /// The pane that last had focus: it titles the tab and its account labels it.
    pub(super) active: usize,
    /// The thread view shown when the tab opened; it shows only with that thread. `None`: with every thread.
    pub owner: Option<EntityId>,
    /// In the dock now (another thread's tabs are taken out while it shows).
    pub docked: bool,
    /// The inline rename field while the title is being renamed.
    pub(super) renaming: Option<(Entity<InputState>, Subscription)>,
    /// Tells this tab from the window's others with the same title: 1 for the first, then the lowest free number.
    pub number: usize,
    /// Runs one command (a Log in): not saved, so a restart doesn't reopen it as a plain shell.
    pub one_shot: bool,
    /// The pane a split just added, while it fades in and grows (Rex's split); `None` once it has settled, so a
    /// pane redrawn elsewhere in the tree (its path changes on the next split) never animates again.
    pub(super) appearing: Option<EntityId>,
    _appear: Task<()>,
    _poll: Task<()>,
}

impl EventEmitter<TabEvent> for TerminalPanel {}

impl TerminalPanel {
    pub fn new(
        pane: Pane,
        name: Option<SharedString>,
        owner: Option<EntityId>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with_tree(vec![pane], None, name, owner, cx)
    }

    /// A tab of `panes` laid out as `tree` (one row in reading order when `None` or when it doesn't name them all).
    pub fn with_tree(
        panes: Vec<Pane>,
        tree: Option<PaneTree<EntityId>>,
        name: Option<SharedString>,
        owner: Option<EntityId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let ids: Vec<EntityId> = panes.iter().map(Pane::id).collect();
        let tree = tree
            .filter(|t| t.leaves().len() == ids.len() && ids.iter().all(|id| t.contains(id)))
            .unwrap_or_else(|| row(&ids));
        let mut this = Self {
            panes,
            tree,
            name,
            active: 0,
            owner,
            docked: true,
            renaming: None,
            number: 1,
            one_shot: false,
            appearing: None,
            _appear: Task::ready(()),
            _poll: Task::ready(()),
        };
        this.order();
        this._poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let Ok(probe) = this.update(cx, |this, cx| this.docked.then(|| this.poll(cx)))
                else {
                    return;
                };
                if let Some(probe) = probe {
                    probe.await;
                }
            }
        });
        this
    }

    /// Asks every shell what it runs and where, off the UI thread; the title and saved folders follow.
    pub(super) fn poll(&self, cx: &mut Context<Self>) -> Task<()> {
        let probes: Vec<_> = self
            .panes
            .iter()
            .map(|p| (p.id(), p.view.read(cx).terminal().probe()))
            .collect();
        let read = cx.background_spawn(async move {
            probes
                .into_iter()
                .map(|(id, probe)| (id, probe.map(|p| p.foreground()).unwrap_or_default()))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let read = read.await;
            let _ = this.update(cx, |this, cx| {
                let mut changed = false;
                for (id, now) in read {
                    let Some(pane) = this.panes.iter_mut().find(|p| p.id() == id) else {
                        continue;
                    };
                    if let Some(cwd) = now.cwd.clone().filter(|c| *c != pane.cwd) {
                        pane.cwd = cwd;
                        changed = true;
                    }
                    if pane.now.command != now.command {
                        changed = true;
                    }
                    pane.now = now;
                }
                if changed {
                    cx.emit(TabEvent::Changed);
                    cx.notify();
                }
            });
        })
    }

    /// The pane holding focus, if any.
    pub fn focused_pane(&self, window: &Window, cx: &App) -> Option<usize> {
        self.panes.iter().position(|p| {
            p.view
                .read(cx)
                .focus_handle(cx)
                .contains_focused(window, cx)
        })
    }

    /// Adds `pane` next to the focused one (or the last), side by side or stacked, and focuses it.
    pub fn split(&mut self, pane: Pane, axis: Axis, window: &mut Window, cx: &mut Context<Self>) {
        let at = self
            .focused_pane(window, cx)
            .unwrap_or(self.panes.len() - 1);
        let (target, id) = (self.panes[at].id(), pane.id());
        let focus = pane.view.read(cx).focus_handle(cx);
        self.tree.split(&target, id, axis);
        self.panes.push(pane);
        self.order();
        self.active = self.index(id).unwrap_or(0);
        if !cx.reduce_motion() {
            self.appearing = Some(id);
            self._appear = cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(crate::ui::motion::SETTLE)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.appearing = None;
                    cx.notify();
                });
            });
        }
        focus.focus(window, cx);
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// Takes the pane with view `id` out and shuts its shell down. False when it was the last pane: the caller closes
    /// the whole tab instead.
    pub fn remove_pane(
        &mut self,
        id: EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.panes.len() < 2 {
            return false;
        }
        let Some(tree) = self.tree.clone().remove(&id) else {
            return false;
        };
        self.tree = tree;
        if let Some(i) = self.index(id) {
            let gone = self.panes.remove(i);
            gone.view.read(cx).terminal().shutdown();
            self.active = i.saturating_sub(1).min(self.panes.len() - 1);
        }
        self.panes[self.active]
            .view
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.emit(TabEvent::Changed);
        cx.notify();
        true
    }

    /// Ends every shell in the tab (it is closing).
    pub fn shutdown(&self, cx: &App) {
        for p in &self.panes {
            p.view.read(cx).terminal().shutdown();
        }
    }

    /// The panes whose shell runs something now (closing them would stop it).
    pub fn busy(&self, ids: &[EntityId], cx: &App) -> Vec<SharedString> {
        self.panes
            .iter()
            .filter(|p| ids.contains(&p.id()))
            .filter(|p| {
                p.view
                    .read(cx)
                    .terminal()
                    .probe()
                    .is_some_and(|probe| probe.is_busy())
            })
            .map(|p| self.pane_title(p))
            .collect()
    }

    /// The tab as it is saved with its window; the workspace fills in its thread's store id.
    pub fn saved(&self) -> SavedTerminalTab {
        SavedTerminalTab {
            thread: None,
            name: self.name.as_ref().map(|n| n.to_string()),
            panes: self.tree.map(&mut |id| {
                self.panes
                    .iter()
                    .find(|p| p.id() == *id)
                    .map(|p| p.cwd.clone())
                    .unwrap_or_default()
            }),
        }
    }

    fn index(&self, id: EntityId) -> Option<usize> {
        self.panes.iter().position(|p| p.id() == id)
    }

    /// Puts `panes` in the tree's reading order, so focus steps and indexes follow what's on screen.
    fn order(&mut self) {
        let order: Vec<EntityId> = self.tree.leaves().into_iter().copied().collect();
        self.panes
            .sort_by_key(|p| order.iter().position(|id| *id == p.id()));
    }

    fn focus_step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.panes.len();
        let at = self.focused_pane(window, cx).unwrap_or(self.active);
        self.active = if forward {
            (at + 1) % n
        } else {
            (at + n - 1) % n
        };
        self.panes[self.active]
            .view
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    pub(super) fn focus_next(
        &mut self,
        _: &FocusNextTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_step(true, window, cx);
    }

    pub(super) fn focus_previous(
        &mut self,
        _: &FocusPreviousTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_step(false, window, cx);
    }
}

/// Panes side by side in the given order.
fn row(ids: &[EntityId]) -> PaneTree<EntityId> {
    let mut tree = PaneTree::Leaf(ids[0]);
    for pair in ids.windows(2) {
        tree.split(&pair[0], pair[1], Axis::Row);
    }
    tree
}

impl Focusable for TerminalPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        let pane = self.panes.get(self.active).or(self.panes.first());
        pane.expect("a terminal tab always has a pane")
            .view
            .read(cx)
            .focus_handle(cx)
    }
}
