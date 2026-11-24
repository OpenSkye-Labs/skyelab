//! A pending question or permission ask docked on top of the composer, after T3's ComposerPendingUserInputPanel: it
//! never scrolls away with the timeline. A question shows one at a time with numbered options; the composer's text is
//! that question's own answer, and Send reads Next, then Submit. A plan stays a card in the timeline, as in T3.

use super::ask_cards::{self, OTHER, questions};
use super::question_drafts;
use super::thread::ThreadView;
use super::thread_model::ApprovalItem;
use crate::ui::{self, Button, Kbd, SkyeIcon, icon_button};
use gpui::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement as _, ParentElement as _,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use gpui_component::{ActiveTheme as _, h_flex, v_flex};
use skyelab_agent::Decision;

const ANSWER_HINT: &str = "Type your own answer, or leave this blank to use the selected option";
const DIGITS: [&str; 9] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
/// How far the composer covers the question panel's bottom edge.
const OVERLAP: gpui::Pixels = px(18.);
const DECLINE_HINT: &str = "Send a message to decline and redirect the agent";

impl ThreadView {
    /// The newest unanswered question or permission ask while the agent can take an answer.
    pub(in crate::workspace) fn dock(&self) -> Option<&ApprovalItem> {
        self.session.as_ref()?;
        self.items.iter().rev().find_map(|i| match i {
            super::thread_model::Item::Approval(a)
                if a.decision.is_none() && a.tool != skyelab_agent::PLAN_TOOL =>
            {
                Some(a)
            }
            _ => None,
        })
    }

    /// The docked question's (step, count), when the dock holds a question.
    pub(in crate::workspace) fn dock_step(&self) -> Option<(usize, usize)> {
        let a = self
            .dock()
            .filter(|a| a.tool == skyelab_agent::QUESTION_TOOL)?;
        let n = questions(&a.raw).len().max(1);
        let step = self.drafts.get(&a.id).map_or(0, |d| d.step);
        Some((step.min(n - 1), n))
    }

    /// The composer's placeholder follows the dock; set only when it changes, so a render never redraws itself.
    pub(in crate::workspace) fn sync_dock_placeholder(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let want = match self.dock() {
            Some(a) if a.tool == skyelab_agent::QUESTION_TOOL => Some(ANSWER_HINT),
            Some(_) => Some(DECLINE_HINT),
            None => None,
        };
        if want == self.composer.dock_placeholder {
            return;
        }
        self.composer.dock_placeholder = want;
        let text = want.unwrap_or(super::thread::PLACEHOLDER);
        self.prompt
            .update(cx, |p, cx| p.set_placeholder(text, window, cx));
    }

    /// Next (or Submit on the last question): the composer's text becomes this question's answer, else its picks do.
    pub(in crate::workspace) fn advance_question(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((step, n)) = self.dock_step() else {
            return;
        };
        let Some(id) = self.dock().map(|a| a.id.clone()) else {
            return;
        };
        let text = self.prompt.read(cx).value().to_string();
        question_drafts::type_answer(&self.items, &mut self.drafts, &id, step, &text);
        let answered = self.drafts.get(&id).is_some_and(|d| {
            d.typed.get(step).is_some_and(|t| !t.is_empty())
                || d.picked
                    .get(step)
                    .is_some_and(|p| p.iter().any(|&o| o != OTHER))
                || d.files.get(step).is_some_and(|f| !f.is_empty())
        });
        if !answered {
            self.composer.notice = Some("Pick an option, or type your answer.".into());
            return cx.notify();
        }
        self.composer.notice = None;
        if step + 1 < n {
            question_drafts::set_step(&self.items, &mut self.drafts, &id, step + 1);
            let next = self
                .drafts
                .get(&id)
                .and_then(|d| d.typed.get(step + 1).cloned())
                .unwrap_or_default();
            self.prompt
                .update(cx, |p, cx| p.set_value(next, window, cx));
            cx.notify();
        } else {
            self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
            self.submit_answers(&id, cx);
        }
    }

    /// Picks option `option` of the docked question; one pick moves on to the next question (never submits).
    fn dock_pick(&mut self, option: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some((step, n)), Some(a)) = (self.dock_step(), self.dock().cloned()) else {
            return;
        };
        let Some(q) = questions(&a.raw).into_iter().nth(step) else {
            return;
        };
        if option >= q.options.len() {
            return;
        }
        let on = self
            .drafts
            .get(&a.id)
            .and_then(|d| d.picked.get(step))
            .is_some_and(|p| p.contains(&option));
        if q.multi || !on {
            self.pick(&a.id, step, option, q.multi, cx);
        }
        if !q.multi && step + 1 < n {
            question_drafts::type_answer(&self.items, &mut self.drafts, &a.id, step, "");
            question_drafts::set_step(&self.items, &mut self.drafts, &a.id, step + 1);
            self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        }
        cx.notify();
    }

    /// A lone digit typed into an empty composer picks that option (T3's 1–9 shortcuts).
    pub(in crate::workspace) fn dock_digit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dock_step().is_none() {
            return;
        }
        let text = self.prompt.read(cx).value();
        let digit = match text.as_bytes() {
            [d @ b'1'..=b'9'] => usize::from(d - b'1'),
            _ => return,
        };
        self.prompt.update(cx, |p, cx| p.set_value("", window, cx));
        self.dock_pick(digit, window, cx);
    }

    /// The panel on the composer's top edge, or `None` with nothing to answer.
    pub(in crate::workspace) fn render_dock(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let a = self.dock()?.clone();
        let question = a.tool == skyelab_agent::QUESTION_TOOL;
        let body = if question {
            self.render_question_dock(&a, window, cx)
        } else {
            ask_cards::render_approval(self, &a, true, cx)
        };
        // A question's panel runs under the composer's top edge, so the two read as one piece (T3's top drawer); an
        // approval card keeps its buttons clear of it.
        Some(
            div()
                .w_full()
                .max_w(ui::CHAT_MAX_W)
                .map(|d| if question { d.mb(-OVERLAP) } else { d.mb_2() })
                .debug_selector(|| "ask-dock".into())
                .child(body)
                .into_any_element(),
        )
    }

    fn render_question_dock(
        &mut self,
        a: &ApprovalItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let qs = questions(&a.raw);
        let (step, n) = self.dock_step().unwrap_or((0, 1));
        let Some(q) = qs.get(step) else {
            return div().into_any_element();
        };
        let picked = self
            .drafts
            .get(&a.id)
            .and_then(|d| d.picked.get(step).cloned())
            .unwrap_or_default();
        let collapsed = self.composer.dock_collapsed.as_deref() == Some(a.id.as_str());
        let typing = !self.prompt.read(cx).value().trim().is_empty();
        let files = self
            .drafts
            .get(&a.id)
            .and_then(|d| d.files.get(step).cloned())
            .unwrap_or_default();
        let attach = super::ask_files::answer_files(&a.id, step, &files, cx).into_any_element();
        let theme = cx.theme();
        let id = a.id.clone();
        let fold_id = a.id.clone();
        let header = h_flex()
            .id("ask-dock-header")
            .gap_1()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                let c = &mut this.composer.dock_collapsed;
                *c = if c.as_deref() == Some(fold_id.as_str()) {
                    None
                } else {
                    Some(fold_id.clone())
                };
                cx.notify();
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(ui::META)
                    .text_color(theme.muted_foreground)
                    .child(if collapsed {
                        format!("{} · {}", q.header, q.question)
                    } else {
                        q.header.clone()
                    }),
            )
            .when(n > 1, |d| {
                d.child(
                    div()
                        .text_size(ui::META)
                        .font_weight(FontWeight::MEDIUM)
                        .child(format!("{}/{n}", step + 1)),
                )
            })
            .child(
                gpui_component::Icon::new(if collapsed {
                    SkyeIcon::ChevronUp
                } else {
                    SkyeIcon::ChevronDown
                })
                .size_3p5()
                .text_color(theme.muted_foreground),
            )
            .child(
                div()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        icon_button(SharedString::from(format!("skip-{id}")), SkyeIcon::Close)
                            .xsmall()
                            .tooltip("Skip the question")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.answer(&id, Decision::Deny, cx)
                            })),
                    ),
            );
        let mut rows = Vec::new();
        for (oi, (label, description)) in q.options.iter().enumerate() {
            let on = !typing && picked.contains(&oi);
            let key = DIGITS.get(oi).and_then(|d| gpui::Keystroke::parse(d).ok());
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("dock-opt-{step}-{oi}")))
                    .debug_selector(move || format!("dock-option-{oi}"))
                    .items_start()
                    .gap_2p5()
                    .py_1p5()
                    .px_1p5()
                    .rounded(ui::RADIUS_MD)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent))
                    .text_color(if on {
                        theme.foreground
                    } else {
                        theme.foreground.opacity(0.7)
                    })
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.dock_pick(oi, window, cx)),
                    )
                    .child(
                        div()
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .child(option_mark(on, q.multi, theme)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(ui::BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label.clone()),
                            )
                            .when(!description.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.muted_foreground)
                                        .child(description.clone()),
                                )
                            }),
                    )
                    .children(
                        key.map(|k| div().h(px(20.)).flex().items_center().child(Kbd::new(k))),
                    ),
            );
        }
        let _ = window;
        v_flex()
            .debug_selector(|| "question-dock".into())
            .w_full()
            .gap_1()
            .px_4()
            .pt_2()
            .pb(px(12.) + OVERLAP)
            .rounded_t(ui::RADIUS_XL)
            .border_1()
            .border_color(theme.border)
            .bg(theme.muted)
            .child(header)
            .when(!collapsed, |d| {
                d.child(
                    div()
                        .pt_0p5()
                        .text_size(ui::BODY)
                        .text_color(theme.foreground.opacity(0.85))
                        .child(q.question.clone()),
                )
                .child(
                    v_flex()
                        .id("ask-dock-options")
                        .max_h(px(320.))
                        .overflow_y_scroll()
                        .children(rows),
                )
                .child(attach)
            })
            .into_any_element()
    }
}

impl ThreadView {
    /// Next or Submit in place of Send while a question is docked.
    pub(in crate::workspace) fn render_dock_send(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (step, n) = self.dock_step()?;
        let label = if step + 1 < n { "Next" } else { "Submit" };
        Some(
            Button::new("dock-next")
                .primary()
                .small()
                .label(label)
                .tooltip(format!("{label} (⌘↩)"))
                .on_click(cx.listener(|this, _, window, cx| this.advance_question(window, cx)))
                .into_any_element(),
        )
    }
}

/// An option's radio (one pick) or checkbox (any).
pub(super) fn option_mark(on: bool, multi: bool, theme: &gpui_component::Theme) -> gpui::Div {
    let base = div()
        .size(px(14.))
        .flex_shrink_0()
        .border_1()
        .border_color(theme.foreground.opacity(0.55))
        .flex()
        .items_center()
        .justify_center();
    if multi {
        base.rounded(ui::RADIUS_SM).when(on, |d| {
            d.bg(theme.primary).border_color(theme.primary).child(
                gpui_component::Icon::new(SkyeIcon::Check)
                    .size_3()
                    .text_color(theme.primary_foreground),
            )
        })
    } else {
        base.rounded_full().when(on, |d| {
            d.bg(theme.primary).border_color(theme.primary).child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(theme.primary_foreground),
            )
        })
    }
}
