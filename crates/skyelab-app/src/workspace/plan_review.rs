//! A plan the agent wrote to a file for review (Antigravity's Plan mode: a `plan.md` artifact asking for feedback),
//! shown as Claude's ExitPlanMode card is. No request waits on it: Approve leaves Plan and asks the agent to go on.

use super::ThreadView;
use crate::workspace::lock;
use crate::workspace::thread_model::Item;
use gpui::{Context, Window};
use skyelab_agent::{Decision, Mode};

/// What Approve sends: the agent ended its turn asking for a review.
pub(in crate::workspace) const APPROVED: &str = "The plan is approved. Implement it now.";

impl ThreadView {
    /// The plan card `id` when it is a plan written for review (its input names the file, `artifact`).
    pub(in crate::workspace) fn review_plan(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|i| {
            matches!(i, Item::Approval(a) if a.id == id && a.tool == skyelab_agent::PLAN_TOOL
                && a.raw.get("artifact").is_some())
        })
    }

    /// Scrolls to the card of the plan written to `path` (a reply's link to it), never reading the file.
    pub(in crate::workspace) fn reveal_plan(&mut self, path: &str, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.items.iter().rposition(|i| {
            matches!(i, Item::Approval(a) if a.tool == skyelab_agent::PLAN_TOOL
                && a.raw["artifact"].as_str() == Some(path))
        }) else {
            return false;
        };
        self.own_list().scroll_to_reveal_item(ix);
        cx.notify();
        true
    }

    /// Approve: the agent leaves Plan (on its stdin ahead of the follow-up, so it carries on in Ask), the card settles
    /// and [`APPROVED`] is sent. Keep planning settles it and hands the composer back.
    pub(in crate::workspace) fn answer_review_plan(
        &mut self,
        id: &str,
        approve: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if approve && self.is_side_chat() {
            let why = crate::workspace::side_chat::PLAN_ONLY;
            return self.show_error(anyhow::anyhow!(why), cx);
        }
        let session = match self.answer_session(cx) {
            Ok(s) => s,
            Err(why) => return self.show_error(anyhow::anyhow!(why), cx),
        };
        let decision = if approve {
            Decision::AllowOnce
        } else {
            Decision::Deny
        };
        let settled = lock(&session).and_then(|mut s| {
            if approve && self.mode == Mode::Plan {
                s.set_mode(Mode::Default)?;
            }
            s.settle_plan_review(id, decision)
        });
        if let Err(e) = settled {
            return self.show_error(e, cx);
        }
        if !approve {
            return self.focus_prompt(window, cx);
        }
        if self.mode == Mode::Plan {
            self.set_mode(Mode::Default, cx);
        }
        if let Err(why) = self.submit_remote(APPROVED, window, cx) {
            self.show_error(anyhow::anyhow!(why), cx);
        }
    }
}
