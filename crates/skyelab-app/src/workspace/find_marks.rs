//! Find in thread's marks on the timeline: the matched words in prompts and replies (markdown range highlights, a
//! prompt's styled text), and a tint on any other row that matches.

use super::thread_items::ItemsView;
use super::thread_model::Item;
use gpui::{AnyElement, Context, Hsla, SharedString, div, prelude::*};
use gpui_component::ActiveTheme as _;
use gpui_component::text::{RangeHighlight, RenderedText, TextViewState};
use skyelab_agent::DeltaKind;
use std::ops::Range;

/// Row `ix` as find shows it: replies and prompts mark the matched words themselves; any other matching row is tinted.
pub(super) fn mark_row<V: ItemsView>(
    this: &V,
    ix: usize,
    row: AnyElement,
    cx: &mut Context<V>,
) -> AnyElement {
    let mark = this.find_words(ix).map(|(_, active)| active.is_some());
    let words = matches!(
        this.items().get(ix),
        Some(
            Item::Text(_)
                | Item::Prompt(_)
                | Item::Steer(_)
                | Item::Live {
                    kind: DeltaKind::Text,
                    ..
                }
        )
    );
    match mark {
        None => row,
        // A find match, the current one stronger.
        Some(active) => div()
            .debug_selector(move || if active { "find-active" } else { "find-match" }.into())
            .w_full()
            .when(!words, |d| {
                d.bg(cx.theme().warning.opacity(if active { 0.22 } else { 0.08 }))
            })
            .child(row)
            .into_any_element(),
    }
}

/// A prompt bubble's text, the find bar's matches marked as in replies.
pub(super) fn prompt_text(
    text: &str,
    find: Option<(&str, Option<usize>)>,
    warning: Hsla,
) -> AnyElement {
    let Some((query, active)) = find else {
        return text.to_owned().into_any_element();
    };
    let marks = word_ranges(text, query)
        .into_iter()
        .enumerate()
        .map(|(n, r)| {
            let opacity = if Some(n) == active { 0.6 } else { 0.25 };
            let style = gpui::HighlightStyle {
                background_color: Some(warning.opacity(opacity)),
                ..Default::default()
            };
            (r, style)
        });
    gpui::StyledText::new(text.to_owned())
        .with_highlights(marks)
        .into_any_element()
}

/// Sets the find highlights on `state` when its parsed text, the query or the current match changed since the last
/// frame (T3 paints the matched words, not the message).
pub(super) fn mark_words<V: 'static>(
    state: &gpui::Entity<TextViewState>,
    find: Option<(&str, Option<usize>)>,
    window: &mut gpui::Window,
    cx: &mut Context<V>,
) {
    type Marked = Option<(RenderedText, Option<(String, Option<usize>)>, Hsla)>;
    let rendered = state.read(cx).rendered_text();
    let want = find.map(|(q, active)| (q.to_owned(), active));
    let warning = cx.theme().warning; // part of the key: a theme switch paints them again
    let id = SharedString::from(format!("{}/find", state.entity_id()));
    let last = window.use_keyed_state(id, cx, |_, _| Marked::None);
    if last.read(cx).as_ref() == Some(&(rendered.clone(), want.clone(), warning)) {
        return;
    }
    state.update(cx, |s, cx| match &want {
        None => s.clear_range_highlights(cx),
        Some((query, active)) => {
            let tint = |n| -> Hsla { warning.opacity(if Some(n) == *active { 0.6 } else { 0.25 }) };
            let marks = word_ranges(rendered.as_str(), query)
                .into_iter()
                .enumerate()
                .map(|(n, r)| RangeHighlight::new(r, tint(n)));
            // The ranges come from this same rendered text, so they are valid.
            // shortcut: find counts a reply's markdown source but marks its rendered text, so a match only in a link
            // target (`[a](a.rs)`) is counted yet unmarked and shifts which word reads as current; count rendered text
            // if that confuses.
            let _ = s.set_range_highlights(marks, cx);
        }
    });
    last.update(cx, |l, _| *l = Some((rendered, want, warning)));
}

/// Where `query` occurs in `text`, case-insensitive, as byte ranges of `text` (non-overlapping, as find counts).
pub(super) fn word_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    // Lowercasing can change a char's length, so each lowercase byte remembers its char's offset in `text`.
    let mut lower = String::with_capacity(text.len());
    let mut at = Vec::with_capacity(text.len() + 1);
    for (i, c) in text.char_indices() {
        for l in c.to_lowercase() {
            at.extend(std::iter::repeat_n(i, l.len_utf8()));
            lower.push(l);
        }
    }
    at.push(text.len());
    lower
        .match_indices(&q)
        .map(|(s, m)| at[s]..at[s + m.len()])
        .filter(|r| !r.is_empty())
        .collect()
}
