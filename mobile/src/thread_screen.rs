//! The thread screen: the agent's events, the requests it waits on, and the composer.

use super::*;

impl MobileApp {
    pub(super) fn event_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        self.rows_built += 1;
        let theme = cx.theme();
        let Some(e) = self.events().get(ix) else {
            return div().into_any_element();
        };
        v_flex()
            .py_2()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.muted_foreground)
                    .child(e.kind.to_uppercase()),
            )
            .child(event_text(e))
            .into_any_element()
    }

    pub(super) fn thread_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let events = list(
            self.events_list.clone(),
            cx.processor(|this, ix, _, cx| this.event_row(ix, cx)),
        )
        .flex_1()
        .min_h_0();
        let asks: Vec<AnyElement> = pending_permissions(self.events())
            .into_iter()
            .map(|(id, what)| self.ask_card(id, what, cx))
            .collect();
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                div().text_sm().text_color(theme.muted_foreground).children(
                    self.thread()
                        .map(|t| format!("{} · {}", t.account_id, t.status)),
                ),
            )
            .child(events)
            .children(asks)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.composer)))
                    .child(
                        Button::new("send")
                            .primary()
                            .label("Send")
                            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx))),
                    )
                    .child(
                        Button::new("stop")
                            .danger()
                            .label("Stop")
                            .on_click(cx.listener(|this, _, _, cx| this.stop(cx))),
                    ),
            )
            .into_any_element()
    }

    /// A request the agent waits on: what it wants, then the three answers.
    fn ask_card(&self, id: String, what: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let answer = |label: &'static str, key: &'static str, d: PermissionAnswer| {
            let id = id.clone();
            Button::new(SharedString::from(format!("{key}-{id}")))
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.answer(&id, d, cx)))
        };
        v_flex()
            .gap_2()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(theme.warning)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Waiting for approval"),
            )
            .child(div().text_sm().child(what.clone()))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        answer("Allow once", "allow-once", PermissionAnswer::AllowOnce).primary(),
                    )
                    .child(answer(
                        "Allow for this thread",
                        "allow-thread",
                        PermissionAnswer::AllowThread,
                    ))
                    .child(answer("Deny", "deny", PermissionAnswer::Deny).ghost()),
            )
            .into_any_element()
    }
}
