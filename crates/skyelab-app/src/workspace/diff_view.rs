//! The timeline's diffs (edit cards, approval previews), after T3's: one line-number gutter, add / remove rows tinted
//! across the whole row with a colored bar and sign, syntax colors by file type, unchanged runs folded to "N unchanged
//! lines". A diff is prepared once (lines, syntax spans) and cached by key, so a frame only builds the rows in view.

use super::changes_model::LineKind;
use super::thread_items::ItemsView;
use crate::ui;
use crate::ui::SkyeIcon;
use crate::ui::syntax::{self, Spans};
use gpui::{
    AnyElement, App, Context, Hsla, Pixels, SharedString, StyledText, div, prelude::*, px,
    uniform_list,
};
use gpui_component::{ActiveTheme as _, Icon, Theme, h_flex};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// A diff line: old and new line numbers, its kind, its text.
pub(super) type Line = (Option<u32>, Option<u32>, LineKind, String);

/// Every row's height, the folded run's too: the card lays its rows out with `uniform_list`.
pub(super) const ROW_H: f32 = 20.;
/// Rows an edit card shows before it scrolls inside; Show all lifts it.
pub(super) const MAX_ROWS: usize = 16;
/// Unchanged lines kept on each side of a change.
pub(super) const CONTEXT: usize = 3;

/// One row a card draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Row {
    Line(usize),
    /// `len` unchanged lines from `start`, folded.
    Gap {
        start: usize,
        len: usize,
    },
}

/// The rows for lines of `kinds`: each unchanged run keeps [`CONTEXT`] lines next to a change and folds the rest
/// (two lines or more; one shows as itself) unless `open(start)`. With no change at all nothing folds.
pub(super) fn rows(kinds: &[LineKind], open: impl Fn(usize) -> bool) -> Vec<Row> {
    let n = kinds.len();
    if kinds.iter().all(|k| *k == LineKind::Context) {
        return (0..n).map(Row::Line).collect();
    }
    let mut out = Vec::with_capacity(n.min(4 * CONTEXT + 64));
    let mut i = 0;
    while i < n {
        if kinds[i] != LineKind::Context {
            out.push(Row::Line(i));
            i += 1;
            continue;
        }
        let end = (i..n).find(|&j| kinds[j] != LineKind::Context).unwrap_or(n);
        let lead = if i == 0 { 0 } else { CONTEXT };
        let trail = if end == n { 0 } else { CONTEXT };
        let (hide_from, hide_to) = (i + lead, end.saturating_sub(trail));
        if hide_to >= hide_from + 2 && !open(hide_from) {
            out.extend((i..hide_from).map(Row::Line));
            out.push(Row::Gap {
                start: hide_from,
                len: hide_to - hide_from,
            });
            out.extend((hide_to..end).map(Row::Line));
        } else {
            out.extend((i..end).map(Row::Line));
        }
        i = end;
    }
    out
}

/// A diff ready to draw: its lines, each line's syntax spans, its counts and its rows with every run folded.
pub(super) struct Prepared {
    pub(super) lines: Vec<(Option<u32>, Option<u32>, LineKind, SharedString)>,
    spans: Vec<Spans>,
    pub(super) added: usize,
    pub(super) removed: usize,
    folded: Rc<Vec<Row>>,
    /// The gutter's width: the widest line number.
    gutter: Pixels,
}

impl Prepared {
    fn new(lines: Vec<Line>, language: &str) -> Self {
        // Each side parsed in order (old: context and removed; new: context and added), so a docstring or block
        // comment colors through, and each line takes its own side's spans.
        let side = |skip: LineKind| {
            let text = lines.iter().filter(|l| l.2 != skip).map(|l| l.3.as_str());
            syntax::highlight(language, text).into_iter()
        };
        let (mut old, mut new) = (side(LineKind::Added), side(LineKind::Removed));
        let spans = lines
            .iter()
            .map(|l| {
                let old = (l.2 != LineKind::Added).then(|| old.next()).flatten();
                let new = (l.2 != LineKind::Removed).then(|| new.next()).flatten();
                if l.2 == LineKind::Removed { old } else { new }.unwrap_or_default()
            })
            .collect();
        let count = |k| lines.iter().filter(|l| l.2 == k).count();
        let widest = lines.iter().filter_map(|l| l.1.or(l.0)).max().unwrap_or(1);
        let kinds: Vec<LineKind> = lines.iter().map(|l| l.2).collect();
        Self {
            added: count(LineKind::Added),
            removed: count(LineKind::Removed),
            folded: Rc::new(rows(&kinds, |_| false)),
            gutter: px(16. + 7.5 * widest.to_string().len() as f32),
            spans,
            lines: lines
                .into_iter()
                .map(|(o, n, k, t)| (o, n, k, t.into()))
                .collect(),
        }
    }

    /// The rows with the runs `open` names unfolded; the cached all-folded list when none is.
    fn rows(&self, open: impl Fn(usize) -> bool) -> Rc<Vec<Row>> {
        let any = self
            .folded
            .iter()
            .any(|r| matches!(r, Row::Gap { start, .. } if open(*start)));
        if !any {
            return self.folded.clone();
        }
        let kinds: Vec<LineKind> = self.lines.iter().map(|l| l.2).collect();
        Rc::new(rows(&kinds, open))
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Rc<Prepared>>> = RefCell::new(HashMap::new());
}

/// The diff cached under `key`, or `lines()` prepared (highlighted for `path`'s language) and cached. The key must
/// change when the lines do.
pub(super) fn prepared(key: &str, path: &str, lines: impl FnOnce() -> Vec<Line>) -> Rc<Prepared> {
    if let Some(p) = CACHE.with(|c| c.borrow().get(key).cloned()) {
        return p;
    }
    let lang = syntax::language(std::path::Path::new(path));
    let p = Rc::new(Prepared::new(lines(), &lang));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        // shortcut: dropped wholesale past 256 diffs, not LRU; a re-highlight is a few ms per card.
        if c.len() >= 256 {
            c.clear();
        }
        c.insert(key.to_owned(), p.clone());
    });
    p
}

/// A row's tint and its gutter's (stronger), with the marker color for the bar, sign and number.
pub(in crate::workspace) fn colors(kind: LineKind, theme: &Theme) -> Option<(Hsla, Hsla, Hsla)> {
    let base = match kind {
        LineKind::Context => return None,
        LineKind::Added => theme.success,
        LineKind::Removed => theme.danger,
    };
    let (row, gutter) = if theme.is_dark() {
        (0.14, 0.26)
    } else {
        (0.10, 0.20)
    };
    Some((
        base.opacity(row),
        base.opacity(gutter),
        ui::ink(theme, base),
    ))
}

/// Line `ix` of `p` as a row; `wrap` lets a long line wrap under itself instead of clipping.
pub(super) fn line_row(p: &Prepared, ix: usize, wrap: bool, theme: &Theme) -> gpui::Div {
    let (old, new, kind, text) = &p.lines[ix];
    let tint = colors(*kind, theme);
    let styles = &theme.highlight_theme;
    let runs = p.spans[ix]
        .iter()
        .filter_map(|(r, name)| Some((r.clone(), styles.style(name)?)));
    let sign = match kind {
        LineKind::Context => " ",
        LineKind::Added => "+",
        LineKind::Removed => "−",
    };
    h_flex()
        .w_full()
        .min_h(px(ROW_H))
        // Each wrapped line one row tall, so a wrapped row measures what it draws.
        .line_height(px(ROW_H))
        .pr_3()
        // In a `max_h` scrolling column a row would shrink below its wrapped text and the next row paint over it.
        .flex_shrink_0()
        .items_start()
        .when_some(tint, |d, t| d.bg(t.0))
        .child(
            div()
                .w(p.gutter)
                .flex_none()
                .min_h(px(ROW_H))
                .pr_2()
                .text_right()
                .border_l(px(3.))
                .border_color(tint.map_or(gpui::transparent_black(), |t| t.2))
                .when_some(tint, |d, t| d.bg(t.1))
                .text_color(tint.map_or(theme.muted_foreground.opacity(0.7), |t| t.2))
                .child(new.or(*old).map(|n| n.to_string()).unwrap_or_default()),
        )
        .child(
            div()
                .w(px(16.))
                .flex_none()
                .text_center()
                .text_color(tint.map_or(theme.muted_foreground, |t| t.2))
                .child(sign),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(theme.foreground)
                .map(|d| {
                    if wrap {
                        d.whitespace_normal()
                    } else {
                        d.whitespace_nowrap().overflow_hidden()
                    }
                })
                .child(StyledText::new(text.clone()).with_highlights(runs)),
        )
}

/// The key a folded run's open state is kept under.
fn gap_key(id: &str, start: usize) -> String {
    format!("{id}:gap:{start}")
}

/// The key a card's Show all is kept under.
pub(super) fn all_key(id: &str) -> String {
    format!("{id}:all")
}

#[cfg(test)]
thread_local! {
    static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many diff rows have been built on this thread, so tests can check only visible ones are.
#[cfg(all(test, unix))] // its test runs on the fake agent, a shell script
pub(super) fn rows_built() -> usize {
    ROWS_BUILT.with(|n| n.get())
}

/// `p`'s rows for a card: only the ones in view are built. Up to [`MAX_ROWS`] tall, then it scrolls, unless Show
/// all (under [`all_key`]) is on. A folded run opens on click. Also gives how many rows there are.
pub(super) fn body<V: ItemsView + 'static>(
    this: &V,
    id: &str,
    p: Rc<Prepared>,
    cx: &mut Context<V>,
) -> (AnyElement, usize) {
    let rows = p.rows(|start| this.is_expanded(&gap_key(id, start)));
    let all = this.is_expanded(&all_key(id));
    let shown = if all {
        rows.len()
    } else {
        rows.len().min(MAX_ROWS)
    };
    let (view, key) = (cx.weak_entity(), id.to_owned());
    let count = rows.len();
    let list = uniform_list(
        SharedString::from(format!("diff-{id}")),
        count,
        move |range, _, cx: &mut App| {
            #[cfg(test)]
            ROWS_BUILT.with(|n| n.set(n.get() + range.len()));
            let theme = cx.theme();
            range
                .filter_map(|i| rows.get(i).copied())
                .map(|row| match row {
                    Row::Line(ix) => line_row(&p, ix, false, theme)
                        .h(px(ROW_H))
                        .into_any_element(),
                    Row::Gap { start, len } => {
                        let (view, key) = (view.clone(), gap_key(&key, start));
                        h_flex()
                            .id(SharedString::from(key.clone()))
                            .debug_selector(|| "diff-gap".into())
                            .h(px(ROW_H))
                            .w_full()
                            .gap_2()
                            .cursor_pointer()
                            .bg(theme.muted.opacity(0.6))
                            .hover(|s| s.bg(theme.accent))
                            .text_color(theme.muted_foreground)
                            .child(
                                div()
                                    .w(p.gutter)
                                    .flex_none()
                                    .flex()
                                    .justify_end()
                                    .pr_2()
                                    .child(Icon::new(SkyeIcon::ChevronsUpDown).size_3()),
                            )
                            .child(format!(
                                "{len} unchanged line{}",
                                if len == 1 { "" } else { "s" }
                            ))
                            .on_click(move |_, _, cx| {
                                view.update(cx, |this, cx| this.toggle_tool(&key, cx)).ok();
                            })
                            .into_any_element()
                    }
                })
                .collect()
        },
    )
    .h(px(ROW_H * shown as f32))
    .w_full()
    .into_any_element();
    (list, count)
}

#[cfg(test)]
#[path = "diff_view_test.rs"]
mod tests;
