//! An AskUserQuestion card's picks before Submit, kept by whichever view shows the card (a local thread or another
//! machine's): which options each question has picked, and the typed "Other" answers.

use super::ask_cards::{OTHER, QuestionDraft, questions};
use super::thread_model::Item;
use gpui::{AppContext as _, Context, Entity, Window};
use gpui_component::input::InputState;
use std::collections::HashMap;

pub(super) type Drafts = HashMap<String, QuestionDraft>;

/// Request `id`'s draft, sized to its questions.
fn draft_mut<'a>(items: &[Item], drafts: &'a mut Drafts, id: &str) -> &'a mut QuestionDraft {
    let n = items
        .iter()
        .find_map(|i| match i {
            Item::Approval(a) if a.id == id => Some(questions(&a.raw).len()),
            _ => None,
        })
        .unwrap_or_default();
    let d = drafts.entry(id.to_owned()).or_default();
    d.picked
        .resize_with(n.max(d.picked.len()), Default::default);
    d.other.resize_with(n.max(d.other.len()), Default::default);
    d
}

/// Toggles a pick. A single-choice question keeps one pick; "Other" is a pick too.
pub(super) fn pick(
    items: &[Item],
    drafts: &mut Drafts,
    id: &str,
    q: usize,
    option: usize,
    multi: bool,
) {
    let Some(set) = draft_mut(items, drafts, id).picked.get_mut(q) else {
        return;
    };
    if !set.remove(&option) {
        if !multi {
            set.clear();
        }
        set.insert(option);
    }
}

/// Question `q`'s "Other" text field, made the first time it shows.
pub(super) fn other_input<V: 'static>(
    items: &[Item],
    drafts: &mut Drafts,
    id: &str,
    q: usize,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Entity<InputState> {
    let existing = draft_mut(items, drafts, id).other.get(q).cloned().flatten();
    if let Some(input) = existing {
        return input;
    }
    let input = cx.new(|cx| InputState::new(window, cx).placeholder("Your answer"));
    if let Some(slot) = draft_mut(items, drafts, id).other.get_mut(q) {
        *slot = Some(input.clone());
    }
    input
}

/// Each question's picks as labels (a typed "Other" answer is its text), or why they can't be sent yet.
pub(super) fn answers(
    items: &[Item],
    drafts: &Drafts,
    id: &str,
    cx: &gpui::App,
) -> Result<Vec<Vec<String>>, String> {
    let qs = items
        .iter()
        .find_map(|i| match i {
            Item::Approval(a) if a.id == id => Some(questions(&a.raw)),
            _ => None,
        })
        .ok_or("that question is gone")?;
    let draft = drafts.get(id);
    qs.iter()
        .enumerate()
        .map(|(qi, q)| {
            let picked = draft.and_then(|d| d.picked.get(qi));
            let mut labels: Vec<String> = picked
                .into_iter()
                .flatten()
                .filter_map(|&o| q.options.get(o).map(|(l, _)| l.clone()))
                .collect();
            if picked.is_some_and(|p| p.contains(&OTHER)) {
                let typed = draft
                    .and_then(|d| d.other.get(qi).cloned().flatten())
                    .map(|i| i.read(cx).value().trim().to_owned())
                    .unwrap_or_default();
                if typed.is_empty() {
                    return Err(format!("Type your answer to \"{}\".", q.question));
                }
                labels.push(typed);
            }
            if labels.is_empty() {
                return Err(format!("Pick an answer to \"{}\".", q.question));
            }
            Ok(labels)
        })
        .collect()
}
