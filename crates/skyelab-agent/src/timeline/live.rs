//! Streaming rows: a long streaming reply is split into rows of whole markdown blocks while it grows, and joined
//! again when it settles.

use super::{DeltaKind, Item, Items, settled};

/// Settles rows still streaming: a block that never completed (the turn failed or was stopped) stays as it
/// streamed, its split parts (see [`split_live`]) joined again. Returns the first row that changed.
pub fn settle_live(items: &mut Items) -> Option<usize> {
    let mut first = None;
    // Rows before the last turn divider were settled when that turn ended; a replayed thread ends hundreds of turns.
    let mut ix = items
        .iter()
        .rposition(|i| matches!(i, Item::Done { .. }))
        .map_or(0, |d| d + 1);
    while ix < items.len() {
        if let Item::Live { kind, .. } = items[ix] {
            let end = live_run_end(items, ix, kind);
            let text = live_text(items, ix..end);
            items.remove(ix + 1..end);
            items[ix] = settled(kind, text, ix);
            first = first.or(Some(ix));
        }
        ix += 1;
    }
    first
}

/// A streaming text row is split once it grows past this many bytes: its whole markdown blocks go into a row of
/// their own, so each delta lays out only the growing tail (and the rows on screen), not the whole reply.
const LIVE_SPLIT: usize = 2048;

/// Splits streaming text row `ix` (the growing tail) after its last whole markdown block, once it is longer than
/// [`LIVE_SPLIT`]: the blocks stay in row `ix`, the rest goes on streaming in a new row after it. The parts are
/// consecutive `Live` rows of one kind, joined again by `finish` and [`settle_live`].
pub(super) fn split_live(items: &mut Items, ix: usize) {
    let Item::Live {
        kind: DeltaKind::Text,
        text,
    } = &mut items[ix]
    else {
        return;
    };
    if text.len() <= LIVE_SPLIT {
        return;
    }
    let Some(cut) = block_cut(text).filter(|c| *c >= LIVE_SPLIT / 2) else {
        return;
    };
    let tail = text.split_off(cut);
    items.insert(
        ix + 1,
        Item::Live {
            kind: DeltaKind::Text,
            text: tail,
        },
    );
}

/// Where `text` can be cut so the part before holds only whole markdown blocks: just after its last blank line that
/// is outside a code fence and followed by a line that starts flush left (an indented one may continue a list item
/// or be indented code). `None` when there is no such line yet.
fn block_cut(text: &str) -> Option<usize> {
    let mut fence: Option<(char, usize)> = None;
    let mut blank_end = None; // the end of the last blank line seen outside a fence
    let mut cut = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        let body = line.trim_start_matches(' ');
        let marker = body.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = marker.map_or(0, |m| body.chars().take_while(|c| *c == m).count());
        match (fence, marker) {
            (Some((m, n)), Some(c)) if c == m && run >= n && body[run..].trim().is_empty() => {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            (None, Some(c)) if run >= 3 && line.len() - body.len() < 4 => {
                if blank_end == Some(start) {
                    cut = Some(start);
                }
                fence = Some((c, run));
                continue;
            }
            _ => {}
        }
        if line.trim().is_empty() {
            if line.ends_with('\n') {
                blank_end = Some(at);
            }
        } else if blank_end == Some(start) && !line.starts_with([' ', '\t']) {
            cut = Some(start);
        }
    }
    cut
}

/// The end of the run of consecutive `Live` rows of `kind` that starts at `start`.
fn live_run_end(items: &[Item], start: usize, kind: DeltaKind) -> usize {
    start
        + items[start..]
            .iter()
            .take_while(|i| matches!(i, Item::Live { kind: k, .. } if *k == kind))
            .count()
}

/// The first row of the run of `Live` rows of `kind` that ends at `last`.
pub(super) fn live_run_start(items: &[Item], last: usize, kind: DeltaKind) -> usize {
    last - items[..last]
        .iter()
        .rev()
        .take_while(|i| matches!(i, Item::Live { kind: k, .. } if *k == kind))
        .count()
}

/// The text of `Live` rows `range`, joined: one streamed block, as it streamed.
pub(super) fn live_text(items: &[Item], range: std::ops::Range<usize>) -> String {
    items[range]
        .iter()
        .filter_map(|i| match i {
            Item::Live { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
