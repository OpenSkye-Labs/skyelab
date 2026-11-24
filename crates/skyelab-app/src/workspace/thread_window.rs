//! Which window a thread view belongs to. One view serves every window that shows the thread (one agent session
//! app-wide); the window that last showed it owns its prompt and its UI side effects.

use super::{SendPrompt, ThreadView};
use gpui::{
    AnyElement, AnyWindowHandle, AppContext as _, Context, Entity, InteractiveElement as _,
    IntoElement as _, ListAlignment, ListState, MouseButton, ParentElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::ActiveTheme as _;
use gpui_component::input::{InputEvent, TextareaState};

pub(in crate::workspace) const PLACEHOLDER: &str =
    "Ask anything. @ files, $ skills, / commands, # threads";

/// The composer's text field, made in `window`: its focus and caret-blink hooks live in that window.
pub(super) fn new_prompt(
    window: &mut Window,
    cx: &mut Context<ThreadView>,
) -> Entity<TextareaState> {
    cx.new(|cx| {
        TextareaState::new(window, cx)
            .auto_grow(2, 10)
            .placeholder(PLACEHOLDER)
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

/// The timeline's list state per window: each window that shows the thread scrolls (and measures rows) on its own,
/// and rows added or changed reach every window's list. Never empty.
pub(in crate::workspace) struct Lists(Vec<(AnyWindowHandle, ListState)>);

/// A chat log: it follows its end (an opened thread shows its latest rows, streamed ones stay in view) until the user
/// scrolls up, and follows again once they scroll back to the end.
pub(in crate::workspace) fn new_list(rows: usize) -> ListState {
    let list = ListState::new(rows, ListAlignment::Top, px(600.));
    list.set_follow_mode(gpui::FollowMode::Tail);
    list
}

impl Lists {
    pub(super) fn new(window: AnyWindowHandle) -> Self {
        Self(vec![(window, new_list(0))])
    }

    /// `window`'s list; a window not added yet draws the first one.
    pub(in crate::workspace) fn of(&self, window: AnyWindowHandle) -> &ListState {
        let first = &self.0[0].1;
        self.0
            .iter()
            .find(|(w, _)| *w == window)
            .map_or(first, |(_, l)| l)
    }

    /// A window that shows the thread gets its own list, starting where the first one is.
    pub(in crate::workspace) fn add(&mut self, window: AnyWindowHandle) {
        if self.0.iter().any(|(w, _)| *w == window) {
            return;
        }
        let first = &self.0[0].1;
        let list = new_list(first.item_count());
        if !first.is_following_tail() {
            list.scroll_to(first.logical_scroll_top());
        }
        self.0.push((window, list));
    }

    /// A closed window's list goes, unless it is the last one (macOS keeps a thread running with no window).
    pub(in crate::workspace) fn remove(&mut self, window: AnyWindowHandle) {
        if self.0.len() > 1 {
            self.0.retain(|(w, _)| *w != window);
        }
    }

    #[cfg(test)]
    pub(in crate::workspace) fn len(&self) -> usize {
        self.0.len()
    }

    pub(in crate::workspace) fn splice(&self, old: std::ops::Range<usize>, count: usize) {
        for (_, l) in &self.0 {
            super::super::timeline_list::splice(l, old.clone(), count);
        }
    }

    /// An ask (permission, question, plan) waits on you: every window's list goes to its end, where it is, even if
    /// you had scrolled up (T3 docks it above the composer).
    pub(in crate::workspace) fn reveal_ask(&self, record: &super::super::thread_model::Record) {
        use super::super::thread_model::Record;
        if matches!(
            record,
            Record::Agent(skyelab_agent::Event::Permission { .. })
        ) {
            for (_, l) in &self.0 {
                l.scroll_to_end();
            }
        }
    }

    /// Where the owner window's reader is, kept so a reopened thread opens there (T3 keeps each thread's scroll):
    /// `None` while it follows the end.
    pub(in crate::workspace) fn mark(&self) -> Option<(usize, f32)> {
        let list = &self.0[0].1;
        (!list.is_following_tail()).then(|| {
            let top = list.logical_scroll_top();
            (top.item_ix, f32::from(top.offset_in_item))
        })
    }

    /// Opens every list at a kept [`Self::mark`]; one past the rows (they changed since) opens at the end.
    pub(in crate::workspace) fn restore(&self, mark: Option<(usize, f32)>) {
        let Some((item_ix, offset)) = mark else {
            return;
        };
        for (_, l) in &self.0 {
            if item_ix < l.item_count() {
                l.scroll_to(gpui::ListOffset {
                    item_ix,
                    offset_in_item: px(offset),
                });
            }
        }
    }

    pub(in crate::workspace) fn reset(&self, rows: usize) {
        for (_, l) in &self.0 {
            l.reset(rows);
        }
    }
}

impl ThreadView {
    /// The list `window` draws the timeline with.
    pub(in crate::workspace) fn list_in(&self, window: AnyWindowHandle) -> &ListState {
        self.lists.of(window)
    }

    /// The owner window's list: where the user typed, searched or sent, so where a scroll for it lands.
    pub(in crate::workspace) fn own_list(&self) -> &ListState {
        self.lists.of(self.window)
    }

    /// The prompt as another window draws it while this view's owner has the live input: its draft as text, and a
    /// click takes the prompt here. Both windows drawing the one input would redraw each other forever (it notifies
    /// whenever its drawn size changes, and two windows give it two sizes).
    pub(in crate::workspace) fn render_borrowed_prompt(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text = self.prompt.read(cx).value();
        let muted = cx.theme().muted_foreground;
        div()
            .id("composer-borrowed")
            .min_h(px(44.))
            .pb_2()
            .cursor_text()
            .whitespace_normal()
            .when(text.is_empty(), |d| d.text_color(muted))
            .child(if text.is_empty() {
                PLACEHOLDER.into()
            } else {
                text
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.attach(window, cx);
                    this.focus_prompt(window, cx);
                }),
            )
            .into_any_element()
    }

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
        self.remote_window = super::super::env_mode::shows_remote(cx, self.window);
        self.composer.dock_placeholder = None; // the new prompt starts with the default one
        self.lists.add(self.window);
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
