//! Keeping a timeline `ListState` steady while its rows change or finish laying out late (docs/gpui/redraw.md).

use gpui::ListState;
use std::ops::Range;

/// How many rows above the first drawn one are measured again on each render.
const SETTLE_ROWS: usize = 8;

/// Rows `old` were replaced by `count` rows. Rows changed in place are measured again, keeping the reader's offset
/// inside the row they are reading (`splice` would put them back at its top); only rows added or removed are spliced.
pub(super) fn splice(list: &ListState, old: Range<usize>, count: usize) {
    let same = old.len().min(count);
    if same > 0 {
        list.remeasure_items(old.start..old.start + same);
    }
    if old.len() != count {
        list.splice(old.start + same..old.end, count - same);
    }
}

/// Rows just above the first drawn one are kept at the height they had when the list measured them in its leading
/// overdraw. A row that finishes laying out later (a reply over 4 KB parses in the background, a diagram or image
/// lands) keeps a stale height there, and the next wheel step into it jumps. Measuring them again on each render
/// fixes their heights before the reader gets there; heights above the first drawn row never move what is shown.
pub(super) fn settle_rows_above(list: &ListState) {
    let top = list.logical_scroll_top().item_ix.min(list.item_count());
    if top > 0 {
        list.remeasure_items(top.saturating_sub(SETTLE_ROWS)..top);
    }
}
