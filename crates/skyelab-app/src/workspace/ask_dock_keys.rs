//! The docked question's keyboard, after T3's shortcuts: it takes the focus when it docks (unless the composer is being
//! typed in), 1–9 pick, ↑/↓ move a one-choice pick, Enter is Next or Submit, Escape goes back to the composer.

use super::questions;
use crate::workspace::thread::ThreadView;
use gpui::{Context, Focusable as _, Window};

/// Focuses the docked question's panel, where 1–9, ↑/↓ and Enter answer it.
pub(in crate::workspace) const FOCUS_QUESTION_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-shift-a"
} else {
    "ctrl-shift-a"
};

gpui::actions!(skyelab, [FocusQuestion]);

impl ThreadView {
    /// A question that just docked takes the focus, so 1–9, ↑/↓ and Enter answer it at once, unless the user is
    /// typing in the composer (T3 leaves keys typed in a text field alone) or works outside this thread. With nothing
    /// focused, only the window's selected thread takes it, not every shown one that got a question too; on a page
    /// with no selected thread (Compare), the first shown pane (a side chat) to get one takes it, and the focus it
    /// takes keeps the others from doing the same.
    pub(super) fn focus_new_question(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held = self
            .composer
            .dock_focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        let Some(id) = self
            .dock_step()
            .and_then(|_| self.dock())
            .map(|a| a.id.clone())
        else {
            // The answered question's panel is gone: its keys go back to the composer, not to nothing.
            if held {
                window.focus(&self.prompt.focus_handle(cx), cx);
            }
            return;
        };
        if self.composer.dock_focused.as_deref() == Some(id.as_str()) {
            return;
        }
        self.composer.dock_focused = Some(id);
        let prompt = self.prompt.read(cx);
        let typing =
            prompt.focus_handle(cx).is_focused(window) && !prompt.value().trim().is_empty();
        let selected = || {
            let shown = cx.try_global::<crate::workspace::center::ShownThread>();
            // A window with no workspace page (a lone thread) has only this thread.
            shown
                .and_then(|s| s.0.get(&window.window_handle().window_id()))
                .is_none_or(|page| page.is_none_or(|id| id == cx.entity_id()))
        };
        let here = held
            || self.focus.contains_focused(window, cx)
            || (window.focused(cx).is_none() && selected());
        if here && !typing {
            self.focus_question(&FocusQuestion, window, cx);
        }
    }

    /// A letter typed while the panel has the focus starts the typed answer: it goes to the composer, which keeps
    /// the focus from then on.
    pub(in crate::workspace) fn dock_type(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = self.prompt.focus_handle(cx);
        window.focus(&focus, cx);
        let text = text.to_owned();
        self.prompt.update(cx, |p, cx| p.insert(text, window, cx));
        cx.notify();
    }

    /// The FocusQuestion action: into the docked question's panel (unfolded, so its options take the keys).
    pub(in crate::workspace) fn focus_question(
        &mut self,
        _: &FocusQuestion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dock_step().is_none() {
            return;
        }
        self.composer.dock_collapsed = None;
        let focus = self
            .composer
            .dock_focus
            .get_or_insert_with(|| cx.focus_handle());
        window.focus(focus, cx);
        cx.notify();
    }

    /// A key pressed while the question panel has the focus: 1–9 pick that option (T3's shortcuts, which leave digits
    /// typed in a field alone), ↑/↓ move a single choice's pick, Enter is Next or Submit, Escape goes back to the
    /// composer. Returns whether it was used.
    pub(in crate::workspace) fn dock_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(id) = self.dock().map(|a| a.id.clone()) else {
            return false;
        };
        if self.composer.dock_collapsed.as_deref() == Some(id.as_str()) {
            return false; // folded away: the options aren't in view
        }
        match key.as_bytes() {
            [d @ b'1'..=b'9'] => {
                let option = usize::from(d - b'1');
                let fits = self.dock_step().is_some_and(|(step, _)| {
                    self.dock()
                        .and_then(|a| questions(&a.raw).into_iter().nth(step))
                        .is_some_and(|q| option < q.options.len())
                });
                if fits {
                    self.dock_pick(option, window, cx);
                }
                fits
            }
            b"enter" => {
                self.advance_question(window, cx);
                true
            }
            b"up" | b"down" => {
                let Some((step, _)) = self.dock_step() else {
                    return false;
                };
                let Some(q) = self
                    .dock()
                    .and_then(|a| questions(&a.raw).into_iter().nth(step))
                else {
                    return false;
                };
                // shortcut: a multi-choice question has no highlight to move, so its arrows do nothing; 1–9 toggle.
                if q.multi || q.options.is_empty() {
                    return false;
                }
                let at = self
                    .drafts
                    .get(&id)
                    .and_then(|d| d.picked.get(step))
                    .and_then(|p| p.iter().next().copied())
                    .filter(|&o| o < q.options.len());
                let last = q.options.len() - 1;
                let next = match (at, key == "down") {
                    (None, true) => 0,
                    (None, false) => last,
                    (Some(o), true) => (o + 1).min(last),
                    (Some(o), false) => o.saturating_sub(1),
                };
                if at != Some(next) {
                    self.pick(&id, step, next, false, cx);
                }
                true
            }
            b"escape" => {
                window.focus(&self.prompt.focus_handle(cx), cx);
                true
            }
            _ => false,
        }
    }
}
