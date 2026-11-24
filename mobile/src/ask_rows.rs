//! The cards for what the agent waits on, above the composer: an AskUserQuestion's options, a proposed plan
//! (Approve / Keep planning), and a permission ask (Allow once / Allow for this thread / Deny). Each goes through the
//! protocol's own call, and only once the connection passed the device and node checks (the client holds every
//! request until then).

use super::*;
use crate::timeline_rows::clip;
use skyelab_agent::timeline::cards::{Question, questions};

/// Each question's picked labels (or typed text, for a question with no options), or why they can't be sent yet.
/// `picked[q]` holds question `q`'s option indexes, `typed[q]` its typed text.
pub fn answers(
    qs: &[Question],
    picked: &[BTreeSet<usize>],
    typed: &[String],
) -> Result<Vec<Vec<String>>, String> {
    qs.iter()
        .enumerate()
        .map(|(qi, q)| {
            if q.options.is_empty() {
                return match typed.get(qi).map(|t| t.trim()) {
                    Some(t) if !t.is_empty() => Ok(vec![t.to_owned()]),
                    _ => Err(format!("Type an answer to \"{}\".", q.question)),
                };
            }
            let labels: Vec<String> = picked
                .get(qi)
                .into_iter()
                .flatten()
                .filter_map(|&o| q.options.get(o).map(|(l, _)| l.clone()))
                .collect();
            if labels.is_empty() {
                return Err(format!("Pick an answer to \"{}\".", q.question));
            }
            Ok(labels)
        })
        .collect()
}

/// The questions of the waiting AskUserQuestion `id`.
fn asked(items: &[Item], id: &str) -> Option<Vec<Question>> {
    items.iter().find_map(|i| match i {
        Item::Approval(a) if a.id == id && a.decision.is_none() => Some(questions(&a.raw)),
        _ => None,
    })
}

impl MobileApp {
    /// Whether this machine takes `answer_questions` (protocol 1.4) over a live, approved connection.
    pub fn can_answer_questions(&self) -> bool {
        self.source
            .as_ref()
            .is_some_and(|s| s.connection() == Connection::Connected && s.speaks(4))
    }

    /// Makes the text field of every waiting question that has no options, the first time it shows.
    pub fn ensure_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open: Vec<(String, usize)> = self
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Approval(a) if a.decision.is_none() && a.tool == "AskUserQuestion" => {
                    Some((a.id.clone(), questions(&a.raw)))
                }
                _ => None,
            })
            .flat_map(|(id, qs)| {
                let free = qs
                    .into_iter()
                    .enumerate()
                    .filter(|(_, q)| q.options.is_empty());
                free.map(move |(qi, _)| (id.clone(), qi))
            })
            .collect();
        for key in open {
            self.typed.entry(key).or_insert_with(|| {
                cx.new(|cx| InputState::new(window, cx).placeholder("Type your answer…"))
            });
        }
    }

    /// The text field of question `q` of request `id`, once it has shown.
    pub fn typed_answer(&self, id: &str, q: usize) -> Option<Entity<InputState>> {
        self.typed.get(&(id.to_owned(), q)).cloned()
    }

    /// Picks option `option` of question `q` (one question: replaces the pick; multi-select: toggles it).
    pub fn pick(&mut self, id: &str, q: usize, option: usize, cx: &mut Context<Self>) {
        let Some(qs) = asked(&self.items, id) else {
            return;
        };
        let multi = qs.get(q).is_some_and(|q| q.multi);
        let draft = self.drafts.entry(id.to_owned()).or_default();
        draft.resize(qs.len(), BTreeSet::new());
        if let Some(set) = draft.get_mut(q) {
            if multi {
                if !set.remove(&option) {
                    set.insert(option);
                }
            } else {
                *set = BTreeSet::from([option]);
            }
        }
        cx.notify();
    }

    /// Sends every question's picks. The card stays until the other machine says the agent took them.
    pub fn submit_answers(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(qs) = asked(&self.items, id) else {
            return;
        };
        let picked = self.drafts.get(id).cloned().unwrap_or_default();
        let typed: Vec<String> = (0..qs.len())
            .map(|q| {
                let field = self.typed_answer(id, q);
                field
                    .map(|f| f.read(cx).value().to_string())
                    .unwrap_or_default()
            })
            .collect();
        match answers(&qs, &picked, &typed) {
            Err(why) => self.error = Some(why.into()),
            Ok(answers) => {
                self.error = None;
                if let (Screen::Thread { thread_id }, true) =
                    (self.screen, self.can_answer_questions())
                    && let Some(source) = self.source.as_mut()
                {
                    source.answer_questions(thread_id, id, answers);
                }
            }
        }
        cx.notify();
    }

    /// Every waiting request's card, in a box that scrolls when they don't fit.
    pub(super) fn ask_cards(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let cards: Vec<AnyElement> = self
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Approval(a) if a.decision.is_none() => Some(a),
                _ => None,
            })
            .map(|a| match a.tool.as_str() {
                "AskUserQuestion" => self.question_card(&a.id, &questions(&a.raw), cx),
                "ExitPlanMode" => {
                    let plan = a.raw["plan"].as_str().unwrap_or_default();
                    self.plan_card(&a.id, plan, cx)
                }
                _ => self.permission_card(&a.id, &format!("{}: {}", a.tool, a.summary), cx),
            })
            .collect();
        (!cards.is_empty()).then(|| {
            v_flex()
                .id("asks")
                .max_h(px(240.))
                .overflow_y_scroll()
                .gap_2()
                .children(cards)
                .into_any_element()
        })
    }

    fn question_card(&self, id: &str, qs: &[Question], cx: &mut Context<Self>) -> AnyElement {
        let answerable = self.can_answer_questions();
        let draft = self.drafts.get(id);
        let mut card = card(cx).child(title("The agent asks", cx));
        for (qi, q) in qs.iter().enumerate() {
            card = card.child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(clip(&q.question)),
            );
            if q.options.is_empty() {
                let field = self.typed_answer(id, qi).filter(|_| answerable);
                card = card.children(field.map(|f| Input::new(&f).h(px(TAP))));
            }
            for (oi, (label, description)) in q.options.iter().enumerate() {
                let on = draft
                    .and_then(|d| d.get(qi))
                    .is_some_and(|s| s.contains(&oi));
                let theme = cx.theme();
                let request = id.to_owned();
                card = card.child(
                    v_flex()
                        .id(SharedString::from(format!("option-{id}-{qi}-{oi}")))
                        .debug_selector(|| "answer-option".into())
                        .min_h(px(TAP))
                        .justify_center()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .border_1()
                        .border_color(if on { theme.primary } else { theme.border })
                        .when(on, |d| d.bg(theme.secondary))
                        .child(
                            div()
                                .text_sm()
                                .child(clip(&format!("{} {label}", if on { "●" } else { "○" }))),
                        )
                        .when(!description.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(clip(description)),
                            )
                        })
                        .when(answerable, |d| {
                            d.on_click(
                                cx.listener(move |this, _, _, cx| this.pick(&request, qi, oi, cx)),
                            )
                        }),
                );
            }
        }
        let request = id.to_owned();
        let footer: AnyElement = if answerable {
            Button::new(SharedString::from(format!("submit-{id}")))
                .primary()
                .debug_selector(|| "answer-submit".into())
                .h(px(TAP))
                .label("Submit")
                .on_click(cx.listener(move |this, _, _, cx| this.submit_answers(&request, cx)))
                .into_any_element()
        } else {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Answer this on that machine: it runs an older SkyeLab.")
                .into_any_element()
        };
        card.child(footer).into_any_element()
    }

    fn plan_card(&self, id: &str, plan: &str, cx: &mut Context<Self>) -> AnyElement {
        let button = |key: &'static str, label: &'static str, d: PermissionAnswer| {
            let request = id.to_owned();
            Button::new(SharedString::from(format!("{key}-{id}")))
                .debug_selector(move || key.into())
                .h(px(TAP))
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.answer(&request, d, cx)))
        };
        let plan: String = plan
            .lines()
            .take(40)
            .map(clip)
            .collect::<Vec<_>>()
            .join("\n");
        card(cx)
            .child(title("Proposed plan", cx))
            .child(div().text_sm().child(plan))
            .child(
                h_flex()
                    .gap_2()
                    .child(button("plan-approve", "Approve", PermissionAnswer::AllowOnce).primary())
                    .child(button("plan-keep", "Keep planning", PermissionAnswer::Deny)),
            )
            .into_any_element()
    }

    /// A tool the agent asks to use: what it wants, then the three answers.
    fn permission_card(&self, id: &str, what: &str, cx: &mut Context<Self>) -> AnyElement {
        let answer = |label: &'static str, key: &'static str, d: PermissionAnswer| {
            let request = id.to_owned();
            Button::new(SharedString::from(format!("{key}-{id}")))
                .debug_selector(move || key.into())
                .h(px(TAP))
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.answer(&request, d, cx)))
        };
        card(cx)
            .child(title("Waiting for approval", cx))
            .child(div().text_sm().child(clip(what)))
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

fn card(cx: &App) -> gpui::Div {
    v_flex()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(cx.theme().warning)
}

fn title(text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.to_uppercase())
}
