//! A finished turn's fold, T3's and Synara's "Worked for 4m 20s ›": the turn's work rows (tools, thinking, output,
//! todos, the text between them) fold behind one header drawn above the first of them. The final reply, questions,
//! approvals and notes stay. A running turn has no fold yet; one that ends in an error result, or with no result
//! (stopped, crashed), never folds.

use super::thread_items::ItemsView;
use super::thread_model::Item;
use super::timeline_text::divider_text;
use crate::ui;
use gpui::{AnyElement, Context, SharedString, div, prelude::*};
use gpui_component::{ActiveTheme as _, Icon, IconName, h_flex, v_flex};
use std::ops::Range;

/// The expanded-set key of the turn whose divider is row `done`. No agent id has this shape.
pub(super) fn fold_key(done: usize) -> String {
    format!("fold:{done}")
}

/// The rows a key's toggle changes: a turn's whole fold, or the one row that expands.
pub(super) fn rows_of(items: &[Item], key: &str) -> Option<Range<usize>> {
    match key.strip_prefix("fold:").and_then(|d| d.parse().ok()) {
        Some(done) if done < items.len() => Some(turn_start(items, done)..done + 1),
        _ => items
            .iter()
            .position(|i| i.expand_id() == Some(key))
            .map(|ix| ix..ix + 1),
    }
}

/// Where the fold of the turn ending at row `done` starts: after the last prompt or divider before it. A turn that was
/// stopped or crashed has no divider, so its rows end at the next prompt and never fold into the turn after it. A
/// steered message is part of the turn, so the fold reaches past it.
pub(super) fn turn_start(items: &[Item], done: usize) -> usize {
    items[..done]
        .iter()
        .rposition(|i| matches!(i, Item::Done { .. } | Item::Prompt(_)))
        .map_or(0, |p| p + 1)
}

/// The rows a fold can hide.
fn work(item: &Item) -> bool {
    matches!(
        item,
        Item::Tool(_) | Item::Thinking { .. } | Item::Output { .. } | Item::Todo(_) | Item::Text(_)
    )
}

/// The turn's reply: its last text, which the fold never hides.
fn reply(items: &[Item], start: usize, done: usize) -> Option<usize> {
    items[start..done]
        .iter()
        .rposition(|i| matches!(i, Item::Text(_)))
        .map(|r| start + r)
}

/// The finished turn's divider row that folds row `ix`, if one does.
// shortcut: walks the turn on each drawn row (only visible rows are drawn); cache the folds per turn if very long
// turns ever scroll slowly.
pub(super) fn folded_by(items: &[Item], ix: usize) -> Option<usize> {
    if !work(items.get(ix)?) {
        return None;
    }
    let done = ix
        + 1
        + items[ix + 1..]
            .iter()
            .position(|i| matches!(i, Item::Done { .. }))?;
    let Item::Done {
        is_error: false, ..
    } = items[done]
    else {
        return None;
    };
    let start = turn_start(items, done);
    (ix >= start && reply(items, start, done) != Some(ix)).then_some(done)
}

/// The first row the turn ending at `done` folds: its header is drawn above it. `None` when nothing folds.
pub(super) fn fold_head(items: &[Item], done: usize) -> Option<usize> {
    let Some(Item::Done {
        is_error: false, ..
    }) = items.get(done)
    else {
        return None;
    };
    let start = turn_start(items, done);
    let reply = reply(items, start, done);
    (start..done).find(|&j| work(&items[j]) && Some(j) != reply)
}

/// The divider of the finished turn whose reply is row `ix`, if it is one: its actions row shows the turn's time.
pub(super) fn reply_of(items: &[Item], ix: usize) -> Option<usize> {
    if !matches!(items.get(ix)?, Item::Text(_)) {
        return None;
    }
    for (j, item) in items.iter().enumerate().skip(ix + 1) {
        match item {
            Item::Done { .. } => return Some(j),
            Item::Text(_) | Item::Live { .. } => return None,
            _ => {}
        }
    }
    None
}

/// Row `ix` as its turn's fold shows it: hidden while folded, under the "Worked for …" header when it is the first
/// folded row, as drawn by `row` otherwise.
pub(super) fn fold_row<V: ItemsView>(
    this: &mut V,
    ix: usize,
    row: impl FnOnce(&mut V, &mut Context<V>) -> AnyElement,
    cx: &mut Context<V>,
) -> AnyElement {
    let items = this.items();
    let Some(done) = folded_by(items, ix) else {
        return row(this, cx);
    };
    let key = fold_key(done);
    let open = this.is_expanded(&key);
    let header = (fold_head(items, done) == Some(ix)).then(|| header(&items[done], open, key, cx));
    let body = open.then(|| row(this, cx));
    v_flex()
        .w_full()
        .children(header)
        .children(body)
        .into_any_element()
}

/// "Worked for 4.2s · 12.3k in · 175 out · $0.0421 ›": opens or folds the turn's work.
fn header<V: ItemsView>(done: &Item, open: bool, key: String, cx: &mut Context<V>) -> AnyElement {
    let Item::Done {
        duration_ms,
        cost_usd,
        usage,
        ..
    } = done
    else {
        return div().into_any_element();
    };
    let (muted, fg) = (cx.theme().muted_foreground, cx.theme().foreground);
    let text = divider_text("Worked for", *duration_ms as f64 / 1000., *usage, *cost_usd);
    let id = SharedString::from(key.clone());
    h_flex()
        .w_full()
        .justify_center()
        .px_6()
        .child(
            div().w_full().max_w(ui::CHAT_MAX_W).pt_2().pb_1().child(
                h_flex()
                    .id(id)
                    .debug_selector(|| "turn-fold".into())
                    .gap_1()
                    .px_1()
                    .text_size(ui::META)
                    .text_color(muted)
                    .cursor_pointer()
                    .hover(move |s| s.text_color(fg))
                    .child(text)
                    .child(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size_3(),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_tool(&key, cx))),
            ),
        )
        .into_any_element()
}
