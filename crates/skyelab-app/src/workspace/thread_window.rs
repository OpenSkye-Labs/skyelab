//! Which window a thread view belongs to. One view serves every window that shows the thread (one agent session
//! app-wide); the window that last showed it owns its prompt and its UI side effects.

use super::{SendPrompt, ThreadView};
use gpui::{AppContext as _, Context, Entity, Subscription, Window};
use gpui_component::input::{InputEvent, TextareaState};

/// The composer's text field, made in `window`: its focus and caret-blink hooks live in that window.
pub(super) fn new_prompt(
    window: &mut Window,
    cx: &mut Context<ThreadView>,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(2, 10)
            .placeholder("Ask anything. @ files, $ skills, / commands, # threads")
    })
}

/// Send on secondary-Enter, and draft handling on every change; runs in `window`.
pub(super) fn prompt_subscription(
    prompt: &Entity<TextareaState>,
    window: &mut Window,
    cx: &mut Context<ThreadView>,
) -> Subscription {
    cx.subscribe_in(
        prompt,
        window,
        |this, _, e: &InputEvent, window, cx| match e {
            InputEvent::PressEnter {
                secondary: true, ..
            } => this.send_prompt(&SendPrompt, window, cx),
            InputEvent::Change => {
                this.prompt_changed(cx);
                this.restore_draft(window, cx);
            }
            _ => {}
        },
    )
}

impl ThreadView {
    /// Whether `window` owns this view: only the owner acts on its events.
    pub(in crate::workspace) fn owned_by(&self, window: &Window) -> bool {
        self.window == window.window_handle()
    }

    /// Makes `window` this view's owner: called when a window shows it, and when its owner window closes. Window-bound
    /// hooks (the prompt's subscription, its focus and blink) die with the window that made them, so they are made
    /// again here, keeping the typed text.
    pub(in crate::workspace) fn attach(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.window == window.window_handle() {
            return;
        }
        self.window = window.window_handle();
        let (text, disabled) = (self.prompt.read(cx).value(), self.read_only);
        let prompt = new_prompt(window, cx);
        prompt.update(cx, |p, cx| {
            p.set_value(text, window, cx);
            p.set_disabled(disabled, cx);
        });
        self.prompt = prompt;
        // The prompt's subscription is always the first.
        self._subs[0] = prompt_subscription(&self.prompt, window, cx);
        // The find bar, branch search and Run menu hold inputs of the old window too: close or make them again.
        self.timeline.find = Default::default();
        let (owner, cwd) = (cx.weak_entity(), self.cwd.clone());
        let (store, project) = (self.backend.store.clone(), self.project_id);
        self.branch_picker = cx.new(|cx| {
            super::super::branch_picker::BranchPicker::new(owner.clone(), cwd, window, cx)
        });
        self.scripts =
            cx.new(|cx| super::super::scripts::ScriptMenu::new(owner, store, project, window, cx));
        cx.notify();
    }
}
