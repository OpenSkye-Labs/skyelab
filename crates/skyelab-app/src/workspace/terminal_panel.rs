//! One terminal tab in the bottom dock: one or more panes side by side (T3's split terminal), each its own shell.

use crate::ui;
use gpui::{
    App, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement, KeyBinding, Render,
    SharedString, Window, actions, div, prelude::*, px,
};
use gpui_component::{ActiveTheme as _, h_flex};
use skyelab_terminal::TerminalView;

actions!(
    skyelab,
    [SplitTerminal, FocusNextTerminal, FocusPreviousTerminal]
);

const MAC: bool = cfg!(target_os = "macos");
// ctrl-d is end-of-file in a shell, hence ctrl-shift-d off macOS.
pub(crate) const SPLIT_TERMINAL_KEY: &str = if MAC { "cmd-d" } else { "ctrl-shift-d" };
pub(crate) const NEXT_TERMINAL_KEY: &str = if MAC {
    "cmd-alt-right"
} else {
    "ctrl-alt-right"
};
pub(crate) const PREVIOUS_TERMINAL_KEY: &str = if MAC { "cmd-alt-left" } else { "ctrl-alt-left" };

/// Binds split and pane focus in a terminal. Call once at startup.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(SPLIT_TERMINAL_KEY, SplitTerminal, Some("Terminal")),
        KeyBinding::new(NEXT_TERMINAL_KEY, FocusNextTerminal, Some("Terminal")),
        KeyBinding::new(
            PREVIOUS_TERMINAL_KEY,
            FocusPreviousTerminal,
            Some("Terminal"),
        ),
    ]);
}

/// A shell in a tab and the account its env was built for (`None`: unbound folder).
pub struct Pane {
    pub view: Entity<TerminalView>,
    pub account: Option<skyelab_core::Account>,
}

pub struct TerminalPanel {
    /// Left to right; never empty.
    pub panes: Vec<Pane>,
    title: SharedString,
    /// The pane that last had focus: its account labels the tab.
    active: usize,
    /// The thread view shown when the tab opened; it shows only with that thread. `None`: with every thread.
    pub owner: Option<EntityId>,
    /// In the dock now (another thread's tabs are taken out while it shows).
    pub docked: bool,
}

impl TerminalPanel {
    pub fn new(pane: Pane, title: SharedString, owner: Option<EntityId>) -> Self {
        Self {
            panes: vec![pane],
            title,
            active: 0,
            owner,
            docked: true,
        }
    }

    pub fn title(&self) -> SharedString {
        self.title.clone()
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

    /// Adds `pane` right after the focused one (or at the end) and focuses it.
    pub fn split(&mut self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) {
        let at = self
            .focused_pane(window, cx)
            .map_or(self.panes.len(), |i| i + 1);
        let focus = pane.view.read(cx).focus_handle(cx);
        self.panes.insert(at, pane);
        self.active = at;
        focus.focus(window, cx);
        cx.notify();
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

    fn focus_next(&mut self, _: &FocusNextTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_step(true, window, cx);
    }

    fn focus_previous(
        &mut self,
        _: &FocusPreviousTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_step(false, window, cx);
    }
}

// shortcut: terminal tabs and panes can't be closed yet (the workspace owns their PTYs); add closing with tab
// management.
impl TerminalPanel {
    /// The tab: its title and the focused pane's account.
    pub fn tab_title(&self) -> impl IntoElement {
        let account = self.panes.get(self.active).and_then(|p| p.account.as_ref());
        h_flex().gap_2().child(self.title.clone()).child(
            account
                .map_or_else(ui::AccountBadge::unbound, ui::AccountBadge::of)
                .chip(),
        )
    }
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

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::debug_renders::count("TerminalPanel");
        if let Some(at) = self.focused_pane(window, cx) {
            self.active = at; // a click moved focus; the tab's account follows it
        }
        let border = cx.theme().border;
        h_flex()
            .size_full()
            .on_action(cx.listener(Self::focus_next))
            .on_action(cx.listener(Self::focus_previous))
            .children(self.panes.iter().enumerate().map(|(i, p)| {
                div()
                    .id(("pane", p.view.entity_id()))
                    .flex_1()
                    .min_w(px(80.))
                    .h_full()
                    .when(i > 0, |d| d.border_l_1().border_color(border))
                    .child(p.view.clone())
            }))
    }
}
