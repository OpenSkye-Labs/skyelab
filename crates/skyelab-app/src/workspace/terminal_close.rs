//! Closing terminals: ⌘W closes the focused pane (its tab with the last one), the tab's × the whole tab. While a
//! shell being closed runs a command, and Settings → General's confirmation is on, a dialog asks first (T3's
//! terminal close confirmation).

use super::Workspace;
use super::terminal_panel::{CloseTerminal, TerminalPanel};
use super::thread_extras::prefs;
use crate::ui::Button;
use gpui::{AppContext as _, Context, Entity, EntityId, SharedString, Window, prelude::*};
use gpui_component::{WindowExt as _, dock::DockPlacement};
use std::cell::RefCell;
use std::rc::Rc;

type Then = Box<dyn FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>)>;

/// The workspace's terminal bookkeeping beyond its tabs.
#[derive(Default)]
pub(super) struct Terms {
    /// The close a confirmation dialog is asking about (its Close runs it).
    pending_close: Option<Rc<RefCell<Option<Then>>>>,
    /// Saved tabs of threads that haven't shown in this window since launch; they open when the thread does.
    pub saved: Vec<skyelab_core::terminal_layout::SavedTerminalTab>,
    /// Saved tabs whose shells are starting: still saved, so a quit meanwhile doesn't lose them.
    pub restoring: Vec<(usize, skyelab_core::terminal_layout::SavedTerminalTab)>,
    pub next_restore: usize,
}

impl Workspace {
    pub(super) fn close_terminal(
        &mut self,
        _: &CloseTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((panel, id)) = self.terminals.iter().find_map(|p| {
            let at = p.read(cx).focused_pane(window, cx)?;
            Some((p.clone(), p.read(cx).panes[at].view.entity_id()))
        }) else {
            return;
        };
        self.close_pane(panel, id, window, cx);
    }

    /// Closes one pane of `panel` (after the confirm, if its shell is busy); the last pane takes the tab with it.
    pub(super) fn close_pane(
        &mut self,
        panel: Entity<TerminalPanel>,
        id: gpui::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = panel.clone();
        self.confirm_terminal_close(&panel, vec![id], window, cx, move |this, window, cx| {
            if !target.update(cx, |p, cx| p.remove_pane(id, window, cx)) {
                this.remove_terminal_tab(&target, window, cx);
            }
        });
    }

    /// The tab's ×: every pane in it.
    pub(super) fn close_terminal_tab(
        &mut self,
        panel: &Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ids = panel
            .read(cx)
            .panes
            .iter()
            .map(|p| p.view.entity_id())
            .collect();
        let target = panel.clone();
        self.confirm_terminal_close(panel, ids, window, cx, move |this, window, cx| {
            this.remove_terminal_tab(&target, window, cx)
        });
    }

    /// Closes multiple terminal tabs together, asking once when any pane runs a command.
    pub(super) fn close_terminal_tabs(
        &mut self,
        panels: Vec<Entity<TerminalPanel>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if panels.is_empty() {
            return;
        }
        let mut busy = Vec::new();
        for panel in &panels {
            let p = panel.read(cx);
            let ids: Vec<EntityId> = p.panes.iter().map(|pane| pane.view.entity_id()).collect();
            busy.extend(p.busy(&ids, cx));
        }
        self.confirm_busy_close(busy, window, cx, move |this, window, cx| {
            for panel in panels {
                if this.terminals.contains(&panel) {
                    this.remove_terminal_tab(&panel, window, cx);
                }
            }
        });
    }

    /// Runs `then` at once, or after the user agrees when a pane in `ids` runs something and confirmation is on.
    fn confirm_terminal_close(
        &mut self,
        panel: &Entity<TerminalPanel>,
        ids: Vec<EntityId>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let busy = panel.read(cx).busy(&ids, cx);
        self.confirm_busy_close(busy, window, cx, then);
    }

    /// Runs `then` at once, or opens the confirmation dialog when `busy` is non-empty and confirmation is on.
    fn confirm_busy_close(
        &mut self,
        busy: Vec<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        if busy.is_empty() || !prefs::get_terminal(cx).confirm_close {
            return then(self, window, cx);
        }
        let (title, body) = close_question(&busy);
        let then: Rc<RefCell<Option<Then>>> = Rc::new(RefCell::new(Some(Box::new(then))));
        self.terms.pending_close = Some(then.clone());
        let ws = cx.entity().downgrade();
        crate::ui::open_dialog(window, cx, move |dialog, _, _| {
            let (ws, then) = (ws.clone(), then.clone());
            dialog
                .w(gpui::px(420.))
                .title(title.clone())
                .child(body.clone())
                .footer(
                    crate::ui::dialog_footer()
                        .child(crate::ui::dialog_cancel("terminal-close-cancel"))
                        .child(
                            Button::new("terminal-close-confirm")
                                .danger()
                                .small()
                                .label("Close")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let Some(then) = then.borrow_mut().take() else {
                                        return;
                                    };
                                    let _ = ws.update(cx, |ws, cx| {
                                        ws.terms.pending_close = None;
                                        then(ws, window, cx)
                                    });
                                }),
                        ),
                )
        });
    }

    /// The confirmation's Close, as the dialog's button runs it (tests).
    #[cfg(all(test, unix))] // its tests run real shells
    pub(super) fn confirm_pending_terminal_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(then) = self
            .terms
            .pending_close
            .take()
            .and_then(|t| t.borrow_mut().take())
        {
            window.close_dialog(cx);
            then(self, window, cx);
        }
    }

    /// Takes a tab out for good: its shells end, it leaves the dock and the saved layout.
    pub(super) fn remove_terminal_tab(
        &mut self,
        panel: &Entity<TerminalPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        panel.read(cx).shutdown(cx);
        let others_shown = self
            .terminals
            .iter()
            .any(|p| p != panel && p.read(cx).docked);
        if panel.read(cx).docked && !others_shown && self.terminals_empty.is_none() {
            // The placeholder goes in first, so the dock never goes empty.
            let empty = cx.new(super::panels::TerminalsEmpty::new);
            self.dock.update(cx, |dock, cx| {
                dock.add_panel_view(
                    gpui_component::dock::panel_handle(empty.clone()),
                    DockPlacement::Bottom,
                    None,
                    window,
                    cx,
                )
            });
            self.terminals_empty = Some(empty);
        }
        if panel.read(cx).docked {
            self.dock
                .update(cx, |dock, cx| dock.remove_panel(panel.clone(), window, cx));
        }
        self.terminals.retain(|p| p != panel);
        // Focus goes to the next shown tab's pane, else back to the page.
        match self.terminals.iter().find(|p| p.read(cx).docked) {
            Some(next) => {
                let focus = gpui::Focusable::focus_handle(next.read(cx), cx);
                focus.focus(window, cx);
            }
            None => self.focus.focus(window, cx),
        }
        self.schedule_layout_save(window, cx);
        cx.notify();
    }
}

/// The dialog's title and text for closing these busy panes.
fn close_question(busy: &[SharedString]) -> (SharedString, SharedString) {
    match busy {
        [one] => (
            format!("Close terminal \"{one}\"?").into(),
            "It is still running. Closing stops the process and clears its history.".into(),
        ),
        many => (
            format!("Close {} terminals?", many.len()).into(),
            format!(
                "They are still running ({}). Closing stops their processes and clears their histories.",
                many.iter()
                    .map(|n| format!("\"{n}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .into(),
        ),
    }
}
