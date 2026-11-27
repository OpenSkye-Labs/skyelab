//! The cards that wait for the user: AskUserQuestion's questions (options, multi-select, "Other") and ExitPlanMode's
//! proposed plan. Their drafts live in the thread view (`thread_answers.rs`).

pub(super) use super::ask_approval::render_approval;
use super::ask_dock::option_mark;
use super::thread_items::ItemsView;
use super::thread_model::ApprovalItem;
use super::tool_cards::slot;
use crate::ui;
use crate::ui::A11yScrollArea as _;
use crate::ui::ActiveTheme as _;
use crate::ui::Button;
use crate::ui::SkyeIcon;
use crate::ui::TextField;
use gpui::{
    AnyElement, Context, FontWeight, IntoElement, SharedString, Window, div, prelude::*, px,
};
use gpui_component::{Icon, h_flex, v_flex};
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
    /// Files attached to each question's answer (T3's question attachments); the agent gets their paths.
    pub(super) files: Vec<Vec<std::path::PathBuf>>,
    /// The docked panel's question (T3 shows one at a time) and each question's answer typed in the composer, which
    /// replaces its picks.
    pub(super) step: usize,
    pub(super) typed: Vec<String>,
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
        .child(slot(SkyeIcon::Check, color))
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

/// A pending ask's line in the timeline while the panel on the composer holds it.
pub(super) fn waiting_row(icon: SkyeIcon, label: &str, cx: &Context<impl ItemsView>) -> AnyElement {
    let theme = cx.theme();
    h_flex()
        .debug_selector(|| "waiting-row".into())
        .min_h(px(28.))
        .gap_1p5()
        .px_1()
        .text_size(ui::BODY)
        .child(slot(icon, ui::ink(theme, theme.warning)))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(ui::META)
                .text_color(ui::ink(theme, theme.warning))
                .child("Waiting for your answer below"),
        )
        .into_any_element()
}

pub(super) fn card(theme: &crate::ui::Theme) -> gpui::Div {
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
    if this.docks_asks(&a.id) {
        return waiting_row(SkyeIcon::Info, "Question", cx);
    }
    let answerable = this.can_answer_questions(cx);
    let attachable = answerable && this.can_attach_answer_files(cx);
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
        let attach = attachable.then(|| {
            let files = this
                .draft(&a.id)
                .and_then(|d| d.files.get(qi).cloned())
                .unwrap_or_default();
            super::ask_files::answer_files(&a.id, qi, &files, cx).into_any_element()
        });
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
            let mark = option_mark(on, q.multi, theme);
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("opt-{}-{qi}-{oi}", a.id)))
                    .role(if q.multi {
                        gpui::accesskit::Role::CheckBox
                    } else {
                        gpui::accesskit::Role::RadioButton
                    })
                    .aria_label(if description.is_empty() {
                        label.clone()
                    } else {
                        format!("{label}, {description}")
                    })
                    .aria_toggled(on.into())
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
                                        .text_size(ui::MICRO)
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
                .children(other.map(|input| {
                    div().pl_7().child(
                        TextField::new(&input)
                            .small()
                            .accessibility_label(format!("{} other answer", q.header)),
                    )
                }))
                .children(attach),
        );
    }
    let actions = if answerable {
        let (submit, skip) = (a.id.clone(), a.id.clone());
        Some(
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
                .into_any_element(),
        )
    } else if this.agent_alive() {
        Some(
            div()
                .text_size(ui::META)
                .text_color(cx.theme().muted_foreground)
                .child("Answer this in the thread on the machine that runs it.")
                .into_any_element(),
        )
    } else {
        // A local thread whose agent ended (a restart, a crash) before the answer: the line over the composer
        // already says so, once.
        None
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
                .child(Icon::new(SkyeIcon::Info).size_3p5())
                .child("The agent has a question"),
        )
        .children(blocks)
        .children(actions)
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
            .debug_selector(|| "plan-actions".into())
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
            // Bordered like its neighbours, so it reads as a button and not a label.
            .child(interrupt(&a.id, cx).outline())
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
                    Icon::new(SkyeIcon::Map)
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
                .a11y_scroll_area()
                .text_size(ui::BODY)
                .child(super::timeline_text::markdown(
                    SharedString::from(format!("plan-md-{}", a.id)),
                    &plan,
                    false,
                    this.files_root(),
                    None,
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
        .tooltip(format!(
            "Stop this turn instead ({})",
            crate::ui::keys("⌘.")
        ))
        .on_click(cx.listener(|this, _, window, cx| this.interrupt_turn(window, cx)))
}
