//! Restart agent session: end the agent so the next send resumes its session in a new process.

use super::{RestartAgentSession, ThreadView};
use crate::ui::Button;
use gpui::{Context, ParentElement as _, Window};

impl ThreadView {
    /// Ends the agent process; the next send resumes the same session in a new one, which loads the account's MCP
    /// servers, skills and plugins as they are now. An agent mid-turn is asked about first, as that cuts the turn off.
    pub(in crate::workspace) fn restart_session(
        &mut self,
        _: &RestartAgentSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.starting {
            // The process starting now reads the account's config as it is: nothing to restart yet.
            return crate::ui::toast::push(
                window,
                crate::ui::toast::Toast::info(
                    "The agent is still starting, and it loads your MCP servers, skills and plugins as they are now.",
                ),
                cx,
            );
        }
        if !self.turn_running() {
            return self.restart_now(window, cx);
        }
        let view = cx.entity().downgrade();
        crate::ui::open_dialog(window, cx, move |dialog, _, _| {
            let view = view.clone();
            dialog
                .w(gpui::px(420.))
                .title("Restart agent session")
                .child("The agent is working. Restarting stops this turn; your next message resumes the session.")
                .footer(
                    crate::ui::dialog_footer()
                        .child(crate::ui::dialog_cancel("restart-cancel"))
                        .child(
                            Button::new("restart-confirm")
                                .primary()
                                .small()
                                .label("Restart")
                                .on_click(move |_, window, cx| {
                                    crate::ui::close_dialog(window, cx);
                                    let _ = view.update(cx, |v, cx| v.restart_now(window, cx));
                                }),
                        ),
                )
        });
    }

    fn restart_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_some() {
            self.stop_requested = true; // `ended` records it stopped, not exited
            self.kill(cx);
        }
        crate::ui::toast::push(
            window,
            crate::ui::toast::Toast::success(
                "Your next message resumes this thread in a new agent process, with your MCP servers, skills and plugins as they are now.",
            )
            .title("Agent session restarted"),
            cx,
        );
        cx.notify();
    }
}
