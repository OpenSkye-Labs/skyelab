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
    d.files.resize_with(n.max(d.files.len()), Default::default);
    d
}

/// The most files one set of answers carries (T3's limit).
const MAX_FILES: usize = 100;

/// Adds `paths` to question `q`'s answer, skipping ones already there and any past [`MAX_FILES`] in all.
pub(super) fn attach(
    items: &[Item],
    drafts: &mut Drafts,
    id: &str,
    q: usize,
    paths: Vec<std::path::PathBuf>,
) {
    let d = draft_mut(items, drafts, id);
    let mut room = MAX_FILES.saturating_sub(d.files.iter().map(Vec::len).sum());
    let Some(files) = d.files.get_mut(q) else {
        return;
    };
    for p in paths {
        if room == 0 {
            break;
        }
        if !files.contains(&p) {
            files.push(p);
            room -= 1;
        }
    }
}

/// Drops file `ix` from question `q`'s answer.
pub(super) fn detach(drafts: &mut Drafts, id: &str, q: usize, ix: usize) {
    if let Some(files) = drafts.get_mut(id).and_then(|d| d.files.get_mut(q))
        && ix < files.len()
    {
        files.remove(ix);
    }
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

/// Each question's picks as labels (a typed "Other" answer is its text, attached files one line of paths), or why
/// they can't be sent yet. Files alone answer a question.
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
            let files = draft
                .and_then(|d| d.files.get(qi))
                .filter(|f| !f.is_empty());
            if picked.is_some_and(|p| p.contains(&OTHER)) {
                let typed = draft
                    .and_then(|d| d.other.get(qi).cloned().flatten())
                    .map(|i| i.read(cx).value().trim().to_owned())
                    .unwrap_or_default();
                match (typed.is_empty(), files.is_some()) {
                    (true, false) => {
                        return Err(format!("Type your answer to \"{}\".", q.question));
                    }
                    (true, true) => {} // "Other" with only files: the files are the answer
                    (false, _) => labels.push(typed),
                }
            }
            if let Some(files) = files {
                // Quoted, so a comma or newline in a file name can't run into the next path.
                let paths: Vec<String> = files
                    .iter()
                    .map(|p| format!("{:?}", p.display().to_string()))
                    .collect();
                labels.push(format!("Attached files: {}", paths.join(", ")));
            }
            if labels.is_empty() {
                return Err(format!("Pick an answer to \"{}\".", q.question));
            }
            Ok(labels)
        })
        .collect()
}
