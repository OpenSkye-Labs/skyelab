//! The cards that wait for the user: AskUserQuestion's questions (options, multi-select, "Other") and ExitPlanMode's
//! proposed plan. Their drafts live in the thread view (`thread_answers.rs`).

use super::thread_items::{ItemsView, clip};
use super::thread_model::ApprovalItem;
use super::tool_cards::slot;
use crate::ui;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    v_flex,
};
use skyelab_agent::Decision;
pub(super) use skyelab_agent::timeline::cards::questions;
use std::collections::BTreeSet;

/// The option index that means "Other" (typed text) in a question draft.
pub(super) const OTHER: usize = usize::MAX;

/// An unanswered AskUserQuestion's picks: per question, the option indexes (or [`OTHER`]) chosen.
#[derive(Default)]
pub(super) struct QuestionDraft {
    pub(super) picked: Vec<BTreeSet<usize>>,
    pub(super) other: Vec<Option<gpui::Entity<gpui_component::input::InputState>>>,
}

/// The label an answered question or plan keeps.
fn settled_label(tool: &str, d: Decision) -> &'static str {
    match (tool, d) {
        (_, Decision::Withdrawn) => "Withdrawn",
        (skyelab_agent::QUESTION_TOOL, Decision::Deny) => "Skipped",
        (skyelab_agent::QUESTION_TOOL, _) => "Answered",
        (_, Decision::Deny) => "Kept planning",
        (_, _) => "Plan approved",
    }
}

fn settled_row(
    a: &ApprovalItem,
    label: &str,
    d: Decision,
    cx: &Context<impl ItemsView>,
) -> AnyElement {
    let theme = cx.theme();
    let color = match d {
        Decision::Withdrawn | Decision::Deny => theme.muted_foreground,
        _ => theme.success,
    };
    h_flex()
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .text_size(ui::BODY)
        .child(slot(IconName::Check, color))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(ui::META)
                .text_color(theme.muted_foreground)
                .child(settled_label(&a.tool, d)),
        )
        .into_any_element()
}

fn card(theme: &gpui_component::Theme) -> gpui::Div {
    v_flex()
        .w_full()
        .gap_2()
        .px_4()
        .py_3()
        .rounded(ui::RADIUS_XL)
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
}

/// AskUserQuestion: each question with its options (radio or checkbox), plus "Other" with a text field; Submit
/// sends every answer.
pub(super) fn render_questions<V: ItemsView>(
    this: &mut V,
    a: &ApprovalItem,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    let qs = questions(&a.raw);
    if let Some(d) = a.decision {
        let title = match qs.as_slice() {
            [one] => one.question.clone(),
            _ => format!("{} questions", qs.len()),
        };
        return settled_row(a, &title, d, cx);
    }
    let answerable = this.can_answer_questions(cx);
    let mut blocks = Vec::new();
    for (qi, q) in qs.iter().enumerate() {
        let picked = this
            .draft(&a.id)
            .and_then(|d| d.picked.get(qi).cloned())
            .unwrap_or_default();
        let other = if answerable && picked.contains(&OTHER) {
            this.other_input(&a.id, qi, window, cx)
        } else {
            None
        };
        let theme = cx.theme();
        let mut rows = Vec::new();
        let choices = q
            .options
            .iter()
            .enumerate()
            .map(|(i, (l, d))| (i, l.clone(), d.clone()))
            .chain(std::iter::once((OTHER, "Other".to_owned(), String::new())));
        for (oi, label, description) in choices {
            let on = picked.contains(&oi);
            let id = a.id.clone();
            let multi = q.multi;
            let mark = if q.multi {
                div()
                    .size(px(14.))
                    .rounded(ui::RADIUS_SM)
                    .border_1()
                    .border_color(theme.foreground.opacity(0.55))
                    .when(on, |d| d.bg(theme.primary).border_color(theme.primary))
                    .child(div().when(on, |d| {
                        d.child(
                            Icon::new(IconName::Check)
                                .size_3()
                                .text_color(theme.primary_foreground),
                        )
                    }))
            } else {
                div()
                    .size(px(14.))
                    .rounded_full()
                    .border_1()
                    .border_color(theme.foreground.opacity(0.55))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(on, |d| {
                        d.bg(theme.primary).border_color(theme.primary).child(
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(theme.primary_foreground),
                        )
                    })
            };
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("opt-{}-{qi}-{oi}", a.id)))
                    .debug_selector(move || format!("question-option-{qi}-{oi}"))
                    .items_start()
                    .gap_2p5()
                    .py_1p5()
                    .px_1p5()
                    .rounded(ui::RADIUS_MD)
                    .when(answerable, |d| {
                        d.cursor_pointer().hover(|s| s.bg(theme.accent))
                    })
                    .text_color(if on {
                        theme.foreground
                    } else {
                        theme.foreground.opacity(0.7)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(&id, qi, oi, multi, cx)))
                    .child(div().h(px(20.)).flex().items_center().child(mark))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(ui::BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label),
                            )
                            .when(!description.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme.muted_foreground)
                                        .child(description),
                                )
                            }),
                    )
                    .into_any_element(),
            );
        }
        blocks.push(
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .text_size(ui::META)
                        .child(
                            div()
                                .px_1p5()
                                .rounded(ui::RADIUS_SM)
                                .bg(theme.secondary)
                                .text_color(theme.muted_foreground)
                                .child(q.header.clone()),
                        )
                        .when(q.multi, |d| {
                            d.child(div().text_color(theme.muted_foreground).child("Pick any"))
                        }),
                )
                .child(
                    div()
                        .text_size(ui::BODY)
                        .text_color(theme.foreground.opacity(0.85))
                        .child(q.question.clone()),
                )
                .children(rows)
                .children(other.map(|input| div().pl_7().child(Input::new(&input).small()))),
        );
    }
    let actions = if answerable {
        let (submit, skip) = (a.id.clone(), a.id.clone());
        h_flex()
            .gap_2()
            .child(
                Button::new(SharedString::from(format!("submit-{}", a.id)))
                    .xsmall()
                    .primary()
                    .label("Submit answers")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.submit_answers(&submit, window, cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("skip-{}", a.id)))
                    .xsmall()
                    .ghost()
                    .label("Skip")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.answer(&skip, Decision::Deny, window, cx)
                    })),
            )
            .child(interrupt(&a.id, cx))
            .into_any_element()
    } else {
        div()
            .text_size(ui::META)
            .text_color(cx.theme().muted_foreground)
            .child("Answer this in the thread on the machine that runs it.")
            .into_any_element()
    };
    let theme = cx.theme();
    card(theme)
        .debug_selector(|| "question-card".into())
        .gap_3()
        .child(
            h_flex()
                .gap_1p5()
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.info))
                .child(Icon::new(IconName::Info).size_3p5())
                .child("The agent has a question"),
        )
        .children(blocks)
        .child(actions)
        .into_any_element()
}

/// ExitPlanMode: the proposed plan as markdown, then Approve or Keep planning.
pub(super) fn render_plan<V: ItemsView>(
    this: &V,
    a: &ApprovalItem,
    window: &mut Window,
    cx: &mut Context<V>,
) -> AnyElement {
    if let Some(d) = a.decision {
        return settled_row(a, "Proposed plan", d, cx);
    }
    let plan = a.raw["plan"].as_str().unwrap_or_default().to_owned();
    let answerable = this.can_answer(cx);
    let actions = answerable.then(|| {
        let (yes, no) = (a.id.clone(), a.id.clone());
        h_flex()
            .gap_2()
            .child(
                Button::new(SharedString::from(format!("approve-{}", a.id)))
                    .xsmall()
                    .primary()
                    .label("Approve")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.answer_plan(&yes, true, window, cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("keep-{}", a.id)))
                    .xsmall()
                    .outline()
                    .label("Keep planning")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.answer_plan(&no, false, window, cx)
                    })),
            )
            .child(interrupt(&a.id, cx))
    });
    let theme = cx.theme();
    card(theme)
        .debug_selector(|| "plan-card".into())
        .child(
            h_flex()
                .gap_1p5()
                .text_size(ui::BODY)
                .font_weight(FontWeight::MEDIUM)
                .child(
                    Icon::new(IconName::Map)
                        .size_4()
                        .text_color(theme.muted_foreground),
                )
                .child("Proposed plan"),
        )
        .child(
            div()
                .id(SharedString::from(format!("plan-{}", a.id)))
                .max_h(px(416.))
                .overflow_y_scroll()
                .text_size(ui::BODY)
                .child(super::timeline_text::markdown(
                    SharedString::from(format!("plan-md-{}", a.id)),
                    &plan,
                    false,
                    this.files_root(),
                    window,
                    cx,
                )),
        )
        .children(actions)
        .into_any_element()
}

/// The approval card's Interrupt: stop the turn instead of answering.
fn interrupt<V: ItemsView>(id: &str, cx: &mut Context<V>) -> Button {
    Button::new(SharedString::from(format!("interrupt-{id}")))
        .xsmall()
        .ghost()
        .label("Interrupt")
        .tooltip("Stop this turn instead (⌘.)")
        .on_click(cx.listener(|this, _, window, cx| this.interrupt_turn(window, cx)))
}

/// The label a decided request keeps.
fn decided(d: Decision) -> &'static str {
    match d {
        Decision::AllowOnce => "Allowed once",
        Decision::AllowThread => "Allowed for this thread",
        Decision::Deny => "Denied",
        Decision::Withdrawn => "Withdrawn",
    }
}

/// A permission request, T3's pending-approval row: what the tool wants to do in mono, then the answers. Once
/// decided it shrinks to one line saying what was decided.
pub(super) fn render_approval<V: ItemsView>(
    this: &V,
    a: &ApprovalItem,
    cx: &mut Context<V>,
) -> AnyElement {
    let theme = cx.theme();
    let summary = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .font_family(theme.mono_font_family.clone())
        .text_size(ui::META)
        .text_color(theme.muted_foreground)
        .child(a.summary.clone());
    if let Some(d) = a.decision {
        let (icon, color) = match d {
            Decision::AllowOnce | Decision::AllowThread => {
                (IconName::Check, theme.muted_foreground)
            }
            Decision::Deny => (IconName::CircleAlert, theme.danger),
            Decision::Withdrawn => (IconName::CircleAlert, theme.muted_foreground),
        };
        return h_flex()
            .min_h(px(28.))
            .gap_1p5()
            .px_1()
            .text_size(ui::BODY)
            .child(
                div()
                    .w(px(24.))
                    .flex()
                    .justify_center()
                    .flex_shrink_0()
                    .child(Icon::new(icon).size_4().text_color(color)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(FontWeight::MEDIUM)
                    .child(a.tool.clone()),
            )
            .child(summary)
            .child(
                div()
                    .text_size(ui::META)
                    .text_color(color)
                    .child(decided(d)),
            )
            .into_any_element();
    }
    let answerable = this.can_answer(cx);
    let theme = cx.theme();
    let button = |label: &'static str, key: &str, decision: Decision| {
        let id = a.id.clone();
        Button::new(SharedString::from(format!("{key}-{}", a.id)))
            .xsmall()
            .label(label)
            .on_click(
                cx.listener(move |this, _, window, cx| this.answer(&id, decision, window, cx)),
            )
    };
    let actions = if answerable {
        h_flex()
            .gap_2()
            .child(button("Allow once", "allow-once", Decision::AllowOnce).primary())
            .child(
                button(
                    "Allow for this thread",
                    "allow-thread",
                    Decision::AllowThread,
                )
                .outline(),
            )
            .child(button("Deny", "deny", Decision::Deny).ghost())
            .child(
                Button::new(SharedString::from(format!("interrupt-{}", a.id)))
                    .xsmall()
                    .ghost()
                    .label("Interrupt")
                    .tooltip("Stop this turn instead (⌘.)")
                    .on_click(cx.listener(|this, _, window, cx| this.interrupt_turn(window, cx))),
            )
            .into_any_element()
    } else {
        div()
            .text_size(ui::META)
            .text_color(theme.muted_foreground)
            .child(
                "The agent isn't running, so this can't be answered. Send a message to continue.",
            )
            .into_any_element()
    };
    let theme = cx.theme();
    v_flex()
        .debug_selector(|| "pending-approval".into())
        .w_full()
        .gap_2()
        .px_3()
        .py_2p5()
        .rounded(ui::RADIUS_LG)
        .bg(theme.warning.opacity(0.08))
        .border_1()
        .border_color(theme.warning.opacity(0.35))
        .child(
            h_flex()
                .gap_1p5()
                .text_size(ui::BODY)
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_4()
                        .text_color(ui::ink(theme, theme.warning)),
                )
                .child(div().font_weight(FontWeight::MEDIUM).child(a.tool.clone()))
                .child(
                    div()
                        .text_size(ui::META)
                        .text_color(ui::ink(theme, theme.warning))
                        .child("Waiting for approval"),
                ),
        )
        .child(
            div()
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::BODY)
                .child(a.summary.clone()),
        )
        .child(
            div()
                .px_3()
                .py_2()
                .rounded(ui::RADIUS)
                .bg(theme.muted)
                .border_1()
                .border_color(theme.border)
                .font_family(theme.mono_font_family.clone())
                .text_size(ui::META)
                .child(clip(&a.input)),
        )
        .child(actions)
        .into_any_element()
}
