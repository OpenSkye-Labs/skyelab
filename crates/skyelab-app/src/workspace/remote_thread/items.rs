//! What the shared timeline renderer and the find bar ask of a remote thread: answering permission requests, questions
//! and plans on the other machine, and the find state.

use super::{RemoteStop, RemoteThreadView};
use crate::environment::ConnState;
use crate::workspace::ask_cards::QuestionDraft;
use crate::workspace::question_drafts;
use crate::workspace::thread_find::{Find, FindHost};
use crate::workspace::thread_items::ItemsView;
use crate::workspace::thread_model::Item;
use gpui::{App, Context, Entity, ListState, Window};

impl ItemsView for RemoteThreadView {
    fn items(&self) -> &crate::workspace::thread_model::Items {
        &self.items
    }
    fn is_expanded(&self, tool_id: &str) -> bool {
        self.expanded.contains(tool_id)
    }
    fn toggle_tool(&mut self, tool_id: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(tool_id) {
            self.expanded.insert(tool_id.to_owned());
        }
        if let Some(rows) = crate::workspace::timeline_fold::rows_of(&self.items, tool_id) {
            let n = rows.len();
            crate::workspace::timeline_list::splice(&self.list, rows, n);
        }
        cx.notify();
    }
    fn interrupt_turn(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(&RemoteStop, window, cx)
    }
    fn can_answer(&self, cx: &App) -> bool {
        self.thread_id.is_some() && self.env.read(cx).state == ConnState::Connected
    }
    fn answer(
        &mut self,
        id: &str,
        decision: skyelab_agent::Decision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use skyelab_client::PermissionAnswer as A;
        let (Some(thread), Some(answer)) = (
            self.thread_id,
            match decision {
                skyelab_agent::Decision::AllowOnce => Some(A::AllowOnce),
                skyelab_agent::Decision::AllowThread => Some(A::AllowThread),
                skyelab_agent::Decision::Deny => Some(A::Deny),
                skyelab_agent::Decision::Withdrawn => None,
            },
        ) else {
            return;
        };
        if !self.can_answer(cx) {
            return;
        }
        // The row updates when the agent's resolved event comes back from the other machine.
        let pending = self.client.answer_permission(thread, id, answer);
        self.await_reply(pending, None, window, cx, |_, _, _| {});
    }
    fn find_words(&self, ix: usize) -> Option<(&str, Option<usize>)> {
        self.find.words(ix)
    }
    /// Only a machine that knows `answer_questions` (1.4) gets the card's controls.
    fn can_answer_questions(&self, cx: &App) -> bool {
        self.can_answer(cx) && self.env.read(cx).speaks(4)
    }
    fn draft(&self, id: &str) -> Option<&QuestionDraft> {
        self.drafts.get(id)
    }
    fn pick(&mut self, id: &str, q: usize, option: usize, multi: bool, cx: &mut Context<Self>) {
        question_drafts::pick(&self.items, &mut self.drafts, id, q, option, multi);
        if let Some(ix) = self
            .items
            .iter()
            .position(|i| matches!(i, Item::Approval(a) if a.id == id))
        {
            crate::workspace::timeline_list::splice(&self.list, ix..ix + 1, 1);
        }
        cx.notify();
    }
    fn other_input(
        &mut self,
        id: &str,
        q: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<crate::ui::TextInput>> {
        Some(question_drafts::other_input(
            &self.items,
            &mut self.drafts,
            id,
            q,
            window,
            cx,
        ))
    }
    /// Sends every answer; the picks stay until the other machine says the agent took them.
    fn submit_answers(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let answers = match question_drafts::answers(&self.items, &self.drafts, id, cx) {
            Ok(a) => a,
            Err(why) => {
                self.error = Some(why.into());
                return cx.notify();
            }
        };
        let Some(thread) = self.thread_id.filter(|_| self.can_answer_questions(cx)) else {
            return;
        };
        self.error = None;
        let pending = self.client.answer_questions(thread, id, answers);
        let id = id.to_owned();
        self.await_reply(pending, None, window, cx, move |this, (), _| {
            this.drafts.remove(&id);
        });
    }
    fn message_menu(
        &mut self,
        is_markdown: bool,
        text: String,
        at: gpui::Point<gpui::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_message_menu(is_markdown, text, at, window, cx);
    }
}

impl FindHost for RemoteThreadView {
    fn find(&self) -> &Find {
        &self.find
    }
    fn find_mut(&mut self) -> &mut Find {
        &mut self.find
    }
    fn list_state(&self) -> &ListState {
        &self.list
    }
    fn focus_after_find(&self, window: &mut Window, cx: &mut App) {
        self.focus_prompt(window, cx);
    }
}
