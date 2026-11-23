//! A finished turn's fold, T3's and Synara's "Worked for 4m 20s ›": the turn's work rows (tools, thinking, output,
//! todos, the text between them) fold behind one header drawn above the first of them. The final reply, questions,
//! approvals and notes stay. A running turn has no fold yet; one that ends in an error result, or with no result
//! (stopped, crashed), never folds. These are the rules; each app draws the header.

use super::Item;
use std::ops::Range;

/// The expanded-set key of the turn whose divider is row `done`. No agent id has this shape.
pub fn fold_key(done: usize) -> String {
    format!("fold:{done}")
}

/// The rows a key's toggle changes: a turn's whole fold, or the one row that expands.
pub fn rows_of(items: &[Item], key: &str) -> Option<Range<usize>> {
    if let Some(done) = key
        .strip_prefix("files:")
        .and_then(|d| d.parse::<usize>().ok())
    {
        return (done < items.len()).then_some(done..done + 1);
    }
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
pub fn turn_start(items: &[Item], done: usize) -> usize {
    items[..done]
        .iter()
        .rposition(|i| matches!(i, Item::Done { .. } | Item::Prompt(_)))
        .map_or(0, |p| p + 1)
}

/// The rows a fold can hide.
fn work(item: &Item) -> bool {
    matches!(
        item,
        Item::Tool(_)
            | Item::Subagent(_)
            | Item::Thinking { .. }
            | Item::Output { .. }
            | Item::Todo(_)
            | Item::Text(_)
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
pub fn folded_by(items: &[Item], ix: usize) -> Option<usize> {
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
pub fn fold_head(items: &[Item], done: usize) -> Option<usize> {
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
pub fn reply_of(items: &[Item], ix: usize) -> Option<usize> {
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
