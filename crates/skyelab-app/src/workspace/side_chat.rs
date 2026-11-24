//! Side chats (T3's `/side` and `/btw`, `SideThreadPane`): a small thread beside the main one, in the right dock. It
//! runs in the parent's folder under the parent's account, as its own agent session whose first prompt starts with
//! the newest part of the parent's conversation, so the parent's agent is never interrupted. Stored as a thread with `side_of` set:
//! hidden from the sidebar, listed in the parent's Details card. Closing the pane keeps it (and a running turn).

use super::thread::composer::composer_context::ChipKind;
use super::thread::{ThreadView, ThreadViewEvent};
use super::thread_model::Item;
use super::{Workspace, lock};
use anyhow::{Result, bail};
use gpui::Global;
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    SharedString, Subscription, WeakEntity, Window, actions, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    dock::{Panel, PanelEvent},
    h_flex, v_flex,
};
use skyelab_core::AccountsConfig;
use skyelab_store::{NewThread, Store, Thread};

#[cfg(test)]
#[path = "side_chat_allow_test.rs"]
mod allow_test;
#[cfg(test)]
#[path = "side_chat_close_test.rs"]
mod close_test;
#[cfg(test)]
#[path = "side_chat_readonly_test.rs"]
mod readonly_test;
#[cfg(test)]
#[path = "side_chat_test.rs"]
mod tests;

actions!(skyelab, [NewSideChat]);

/// The window each side chat's pane lives in: one view per side chat, so two windows never run two agents on it.
#[derive(Default)]
struct SideChatWindows(std::collections::HashMap<i64, gpui::AnyWindowHandle>);

impl Global for SideChatWindows {}

/// What a thread asks of the workspace.
#[derive(Debug, Clone, PartialEq)]
pub enum SideAsk {
    /// A new side chat, sending its first question when there is one.
    New(Option<String>),
    /// A stored side chat, by id.
    Open(i64),
}

/// `/side <question>` or `/btw <question>`: `Some(question)` (`None` for the bare command); anything else is a
/// message for the thread.
pub(super) fn side_question(text: &str) -> Option<Option<String>> {
    let rest = ["/side", "/btw"]
        .iter()
        .find_map(|c| text.strip_prefix(c))?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // `/sidebar` is another command
    }
    let q = rest.trim();
    Some((!q.is_empty()).then(|| q.to_owned()))
}

/// The side chat's title: its question's first line, cut short.
fn title(question: Option<&str>) -> String {
    let line = question
        .and_then(|q| q.lines().next())
        .unwrap_or_default()
        .trim();
    match line.chars().count() {
        0 => "Side chat".into(),
        n if n > 48 => format!("Side: {}…", line.chars().take(48).collect::<String>()),
        _ => format!("Side: {line}"),
    }
}

/// Stores a side chat of `parent` in its folder under its account, with the summary of its conversation. Refused,
/// creating nothing, when the folder no longer resolves to that account (rebound, or unbound).
pub(super) fn create(
    store: &mut Store,
    config: &AccountsConfig,
    parent: &Thread,
    title: &str,
) -> Result<Thread> {
    match config.resolve(&parent.cwd) {
        Some(a) if a.id == parent.account_id => {}
        _ => bail!(
            "{} isn't bound to {} any more, so a side chat can't start there",
            parent.cwd.display(),
            parent.account_id
        ),
    }
    let events = store.events_since(parent.id, 0)?;
    let context = crate::workspace::thread_extras::fork::side_prompt(&parent.title, &events);
    let new = NewThread {
        project_id: parent.project_id,
        title,
        cwd: &parent.cwd,
        account_id: &parent.account_id,
        status: "stopped",
    };
    store.create_side_chat(&new, parent.id, &context)
}

/// Why a side chat's mode can't change, and its plans can't be approved.
pub(in crate::workspace) const PLAN_ONLY: &str = "A side chat stays in Plan mode: it answers questions and never \
     changes files. Ask the main thread to make changes.";

/// The mode a stored thread opens in: its own, or Plan for a side chat (rows stored before side chats were Plan-only
/// say Ask).
pub(in crate::workspace) fn row_mode(row: &Thread) -> skyelab_agent::Mode {
    match row.side_of {
        Some(_) => skyelab_agent::Mode::Plan,
        None => skyelab_agent::Mode::parse(&row.permission_mode),
    }
}

impl ThreadView {
    pub(in crate::workspace) fn is_side_chat(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| t.side_of.is_some())
    }

    /// Why `/side` can't start here, checked before the question leaves the composer (the workspace checks again,
    /// on the accounts file as saved now).
    pub(in crate::workspace) fn side_refusal(&self) -> Option<String> {
        let Some(row) = &self.thread else {
            return Some(
                "Send a message first: a side chat starts from the thread's conversation.".into(),
            );
        };
        match self.config.resolve(&self.cwd) {
            Some(a) if a.id == row.account_id => None,
            _ => Some(format!(
                "This folder isn't bound to {} any more, so a side chat can't start here.",
                row.account_id
            )),
        }
    }

    pub(in crate::workspace) fn new_side_chat(
        &mut self,
        _: &NewSideChat,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_side_chat() {
            cx.emit(ThreadViewEvent::Side(SideAsk::New(None)));
        }
    }

    /// The last reply's text, for "Bring back".
    pub(in crate::workspace) fn last_answer(&self) -> Option<&str> {
        self.items.iter().rev().find_map(|i| match i {
            Item::Text(t) if !t.trim().is_empty() => Some(t.as_str()),
            _ => None,
        })
    }
}

/// A side chat in the right dock: its title, "Bring back", and the chat itself without its page header.
pub struct SideChatPanel {
    focus: FocusHandle,
    pub(super) id: i64,
    /// The parent thread's row.
    parent_id: i64,
    pub(super) parent: WeakEntity<ThreadView>,
    pub(super) view: Entity<ThreadView>,
    /// The workspace's handler for what the chat asks of it (a file link opens the Preview tab).
    _events: Subscription,
}

impl SideChatPanel {
    /// Puts the side chat's last answer in the parent's composer as a chip, under the side chat's own account and
    /// folder (refused there when the composer's account or the binding differs).
    pub(in crate::workspace) fn bring_back(&mut self, cx: &mut Context<Self>) {
        let v = self.view.read(cx);
        let (Some(answer), Some(row)) = (v.last_answer(), v.thread.as_ref()) else {
            return;
        };
        let text = skyelab_core::terminal_context::clip(answer);
        let label: SharedString = row.title.clone().into();
        let bound = Some((row.account_id.clone(), row.cwd.clone()));
        let _ = self.parent.update(cx, |p, cx| {
            p.push_context(ChipKind::SideChat, label, text, bound, cx)
        });
    }
}

impl EventEmitter<PanelEvent> for SideChatPanel {}

impl gpui_component::dock::BasePanel for SideChatPanel {
    fn panel_name(&self) -> &'static str {
        "SideChat"
    }
    fn closable(&self, _: &App) -> bool {
        true
    }
    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for SideChatPanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.view.read(cx).title()
    }
    fn zoom_control(&self, _: &App) -> Option<gpui_component::dock::PanelControl> {
        None
    }
}

impl Focusable for SideChatPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SideChatPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let _render = crate::debug_renders::count("SideChatPanel");
        let v = self.view.read(cx);
        let title = v.title();
        let answered = v.last_answer().is_some();
        let theme = cx.theme();
        // T3's side chat header: 36 tall, the title in xs medium, its actions on the right.
        let header = h_flex()
            .h(px(36.))
            .flex_shrink_0()
            .pl_3()
            .pr_1()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                Button::new("side-chat-bring-back")
                    .ghost()
                    .xsmall()
                    .label("Bring back")
                    .disabled(!answered)
                    .tooltip(if answered {
                        "Adds the last answer to the main thread's next message."
                    } else {
                        "Available after the first answer."
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.bring_back(cx))),
            );
        v_flex()
            .id(("side-chat", self.id as u64))
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .child(header)
            .child(div().flex_1().min_h_0().child(self.view.clone()))
    }
}

impl Workspace {
    /// A thread asked for a side chat: a new one in its folder under its account, or a stored one reopened.
    pub(super) fn side_chat(
        &mut self,
        parent: &Entity<ThreadView>,
        ask: SideAsk,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_remote() {
            return; // side chats are this machine's only
        }
        let Some(row) = parent
            .read(cx)
            .thread
            .clone()
            .filter(|t| t.side_of.is_none())
        else {
            return self.say(
                "Send a message first: a side chat starts from the thread's conversation.",
                window,
                cx,
            );
        };
        let question = match ask {
            SideAsk::Open(id) => return self.reopen_side_chat(parent, id, window, cx),
            SideAsk::New(q) => q,
        };
        let (store, config) = (self.backend.store.clone(), self.accounts_job(cx));
        let name = title(question.as_deref());
        let task =
            cx.background_spawn(async move { create(&mut *lock(&store)?, &config(), &row, &name) });
        let parent = parent.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let made = task.await;
            let _ = this.update_in(cx, |ws, window, cx| match (made, parent.upgrade()) {
                (Ok(side), Some(parent)) => {
                    parent.update(cx, |p, cx| {
                        p.details
                            .side_chats
                            .insert(0, (side.id, side.title.clone().into()));
                        cx.notify();
                    });
                    let panel = ws.side_panel(side, &parent, window, cx);
                    if let (Some(q), Some(panel)) = (question, panel) {
                        ws.ask_when_ready(&panel.read(cx).view.clone(), q, window, cx);
                    }
                }
                (Ok(_), None) => {}
                (Err(e), _) => ws.say(format!("Couldn't start a side chat: {e:#}"), window, cx),
            });
        })
        .detach(); // one short store write; `this` is weak
    }

    /// Shows a stored side chat of `parent` again: its kept pane, or a new one on its row.
    fn reopen_side_chat(
        &mut self,
        parent: &Entity<ThreadView>,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.backend.store.clone();
        let row = lock(&store).and_then(|s| s.thread(id)).ok().flatten();
        let Some(side) = row.filter(|t| t.side_of == parent.read(cx).thread.as_ref().map(|p| p.id))
        else {
            return self.say("That side chat is gone.", window, cx);
        };
        self.side_panel(side, parent, window, cx);
    }

    /// The side chat's pane in the right dock, created once and kept, then shown. `None` when another open window
    /// has it.
    fn side_panel(
        &mut self,
        side: Thread,
        parent: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<SideChatPanel>> {
        let here = self.window_handle;
        let owner = cx
            .default_global::<SideChatWindows>()
            .0
            .get(&side.id)
            .copied();
        if owner.is_some_and(|w| w != here && cx.windows().contains(&w)) {
            self.say("This side chat is open in another window.", window, cx);
            return None;
        }
        cx.default_global::<SideChatWindows>()
            .0
            .insert(side.id, here);
        let panel = match self.side_chats.iter().find(|p| p.read(cx).id == side.id) {
            Some(p) => {
                // Its parent may have closed and opened again since: Bring back goes to the view showing it now.
                let parent = parent.downgrade();
                p.update(cx, |p, _| p.parent = parent);
                p.clone()
            }
            None => {
                let (config, backend) = (self.config.clone(), self.backend.clone());
                let (id, parent_id) = (side.id, side.side_of.unwrap_or_default());
                let view = cx.new(|cx| ThreadView::open(side, config, backend, window, cx));
                // Files its replies and cards name open in the Preview tab: a side chat changes none to review.
                let events = cx.subscribe_in(&view, window, |ws, view, e, window, cx| {
                    let (path, line) = match e {
                        ThreadViewEvent::Preview(path, line) => (path, *line),
                        ThreadViewEvent::OpenPath(path) => (path, None),
                        _ => return,
                    };
                    let root = view.read(cx).cwd.clone();
                    ws.open_preview(root, path.clone(), line, window, cx)
                });
                let panel = cx.new(|cx| SideChatPanel {
                    focus: cx.focus_handle(),
                    id,
                    parent_id,
                    parent: parent.downgrade(),
                    view,
                    _events: events,
                });
                self.side_chats.push(panel.clone());
                panel
            }
        };
        // A closed pane left the dock but stays here, so it goes back in.
        let id = panel.entity_id().into();
        if self.dock.read(cx).panel(id).is_none() {
            self.add_right_panel(panel.clone(), window, cx);
        }
        self.show_right_panel(id, window, cx);
        Some(panel)
    }

    /// Settings saved: side chats check against the new bindings too.
    pub(super) fn push_side_config(&self, cx: &mut Context<Self>) {
        for p in &self.side_chats {
            let (view, config) = (p.read(cx).view.clone(), self.config.clone());
            view.update(cx, |v, cx| v.set_config(config, cx));
        }
    }

    /// The thread `view` closed (archived, deleted, its project removed): its side chats stop and leave the dock.
    pub(super) fn close_side_chats(
        &mut self,
        view: &Entity<ThreadView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = view.read(cx).thread.as_ref().map(|t| t.id) else {
            return;
        };
        let (gone, kept) = std::mem::take(&mut self.side_chats)
            .into_iter()
            .partition(|p| p.read(cx).parent_id == parent);
        self.side_chats = kept;
        for panel in gone {
            let (id, side) = (panel.read(cx).id, panel.read(cx).view.clone());
            side.update(cx, |v, _| v.close());
            cx.default_global::<SideChatWindows>().0.remove(&id);
            self.dock
                .update(cx, |dock, cx| dock.remove_panel(panel, window, cx));
        }
    }

    /// Sends `question` in the side chat once its row has loaded (it is read-only until then).
    fn ask_when_ready(
        &mut self,
        view: &Entity<ThreadView>,
        question: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = window.window_handle();
        let mut question = Some(question);
        self.wait_on(view, cx, move |_, view, cx| {
            if view.read(cx).read_only {
                return false;
            }
            if let Some(q) = question.take() {
                // The observer has no Window, and sending needs one.
                cx.defer(move |cx| {
                    let _ = handle.update(cx, |_, window, cx| {
                        view.update(cx, |v, cx| v.submit_remote(&q, window, cx))
                    });
                });
            }
            true
        });
    }
}
