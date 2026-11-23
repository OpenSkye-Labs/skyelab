//! A remote device's answers to what an agent here waits on: a permission request, an ExitPlanMode plan (an
//! approval also takes the thread out of Plan, as a click on its card does) and an AskUserQuestion. Each is replied
//! to once the agent took it, or with why it didn't.

use super::Workspace;
use super::remote::{decision_of, not_open, refused};
use crate::remote::Reply;
use gpui::Context;
use skyelab_protocol::PermissionAnswer;

impl Workspace {
    pub(super) fn remote_answer(
        &mut self,
        thread_id: i64,
        request_id: &str,
        decision: PermissionAnswer,
        reply: Reply<()>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.open_view(thread_id, cx) else {
            let _ = reply.send(Err(not_open(thread_id)));
            return;
        };
        let late = reply.clone();
        let done = move |r: Result<(), String>| _ = late.send(r.map_err(refused));
        let asked = view.update(cx, |v, cx| {
            if v.is_plan(request_id) {
                v.answer_plan_then(request_id, decision != PermissionAnswer::Deny, done, cx)
            } else {
                v.answer_then(request_id, decision_of(decision), done, cx)
            }
        });
        if let Err(why) = asked {
            let _ = reply.send(Err(refused(why)));
        }
    }

    pub(super) fn remote_questions(
        &mut self,
        thread_id: i64,
        request_id: &str,
        answers: Vec<Vec<String>>,
        reply: Reply<()>,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.open_view(thread_id, cx) else {
            let _ = reply.send(Err(not_open(thread_id)));
            return;
        };
        let late = reply.clone();
        let asked = view.update(cx, |v, cx| {
            v.answer_questions_then(
                request_id,
                answers,
                move |r| _ = late.send(r.map_err(refused)),
                cx,
            )
        });
        if let Err(why) = asked {
            let _ = reply.send(Err(refused(why)));
        }
    }
}

#[cfg(test)]
impl Workspace {
    /// The open thread's permission mode.
    pub(crate) fn thread_mode(&self, id: i64, cx: &gpui::App) -> Option<skyelab_agent::Mode> {
        Some(self.open_view(id, cx)?.read(cx).mode)
    }

    /// Puts the open thread in `mode` without telling its agent (as a thread started in that mode would be).
    pub(crate) fn put_thread_in_mode(
        &self,
        id: i64,
        mode: skyelab_agent::Mode,
        cx: &mut gpui::App,
    ) {
        if let Some(view) = self.open_view(id, cx) {
            view.update(cx, |v, _| v.mode = mode);
        }
    }
}
