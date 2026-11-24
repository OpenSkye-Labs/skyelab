//! Drawing the Changes tab's rows: the diff (unified or side by side, clipped or wrapped, find matches tinted) and
//! the file tree beside it.

use super::ChangesView;
use crate::ui;
use crate::ui::Checkbox;
use crate::ui::SkyeIcon;
use crate::workspace::changes_model::{LineKind, Row};
use crate::workspace::changes_tree::{SplitRow, TreeRow};
use gpui::{AnyElement, App, Context, FontWeight, Hsla, SharedString, Window, div, prelude::*, px};
use gpui_component::{ActiveTheme as _, Icon, Sizable as _, h_flex};
use skyelab_git::ChangeKind;
use std::ops::Range;

const ROW_HEIGHT: f32 = 20.;
/// A line number's column and the sign's: the gutter a click comments from spans them.
const NUM_W: f32 = 44.;
const SIGN_W: f32 = 14.;
/// The file tree's rows, T3's 28.
const FILE_ROW_HEIGHT: f32 = 28.;
/// Indent per tree level.
const TREE_INDENT: f32 = 12.;

impl ChangesView {
    /// Item `ix` of the diff list: a unified row, or a side-by-side one in split mode.
    pub(super) fn render_item(
        &mut self,
        ix: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(changes) = &self.changes else {
            return div().into_any_element();
        };
        // The unified rows this item shows (annotations ending there go under it), and its old and new side's rows.
        let over = match self.split.get(ix) {
            Some(SplitRow::Pair(l, r)) if self.prefs.split => Some((*l, *r)),
            Some(SplitRow::Full(i)) if self.prefs.split => Some((Some(*i), Some(*i))),
            _ if self.prefs.split => None,
            _ => Some((Some(ix), Some(ix))),
        };
        let (shown, halves): (Vec<usize>, Vec<(usize, bool)>) = match self.split.get(ix) {
            Some(SplitRow::Pair(l, r)) if self.prefs.split => {
                let sides = [(*l, true), (*r, false)];
                let sides: Vec<(usize, bool)> = sides
                    .into_iter()
                    .filter_map(|(i, left)| Some((i?, left)))
                    .collect();
                (sides.iter().map(|s| s.0).collect(), sides)
            }
            Some(SplitRow::Full(i)) if self.prefs.split => (vec![*i], Vec::new()),
            _ if self.prefs.split => (Vec::new(), Vec::new()),
            _ => (vec![ix], Vec::new()),
        };
        let is_line = |i: usize| matches!(changes.rows.get(i), Some(Row::Line { .. }));
        let unified_gutter = (halves.is_empty())
            .then(|| shown.first().copied().filter(|&i| is_line(i)))
            .flatten()
            .map(|i| self.gutter(i, 2. * NUM_W + SIGN_W, false, cx));
        let mut gutters: Vec<(bool, Option<AnyElement>)> = halves
            .iter()
            .map(|&(i, left)| (left, Some(self.gutter(i, NUM_W + SIGN_W, left, cx))))
            .collect();
        let picked = shown.iter().any(|&i| self.picked(i));
        let notes = self.annotations(|i| shown.contains(&i), cx);
        let theme = cx.theme();
        let wrap = self.prefs.wrap;
        let base = h_flex()
            .id(("diff-row", ix))
            .relative()
            .w_full()
            .items_start()
            .font_family(theme.mono_font_family.clone())
            .text_xs()
            .map(|d| {
                if wrap {
                    d.min_h(px(ROW_HEIGHT))
                } else {
                    d.h(px(ROW_HEIGHT)).overflow_hidden().whitespace_nowrap()
                }
            });
        let (row, mark) = if self.prefs.split {
            match self.split.get(ix) {
                Some(SplitRow::Pair(l, r)) => {
                    let mark = [*l, *r]
                        .into_iter()
                        .flatten()
                        .filter_map(|i| self.find.mark(i))
                        .max();
                    let mut half = |i: Option<usize>, left: bool| {
                        let line = i.and_then(|i| match changes.rows.get(i) {
                            Some(Row::Line {
                                old,
                                new,
                                kind,
                                text,
                            }) => Some((*old, *new, *kind, text.clone())),
                            _ => None,
                        });
                        let el = div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .overflow_hidden();
                        let el = el.children(
                            gutters
                                .iter_mut()
                                .find(|(l, _)| *l == left)
                                .and_then(|(_, g)| g.take()),
                        );
                        match line {
                            Some((old, new, kind, text)) => {
                                let n = if left { old } else { new };
                                el.child(line_parts(
                                    h_flex().w_full().items_start(),
                                    n,
                                    None,
                                    kind,
                                    text.into(),
                                    wrap,
                                    theme,
                                ))
                            }
                            None => el.bg(theme.muted.opacity(0.5)),
                        }
                    };
                    let row = base
                        .child(half(*l, true))
                        .child(div().w(px(1.)).flex_none().h_full().bg(theme.border))
                        .child(half(*r, false));
                    (row, mark)
                }
                Some(SplitRow::Full(i)) => (self.full_row(base, *i, cx), self.find.mark(*i)),
                None => return div().into_any_element(),
            }
        } else {
            (self.full_row(base, ix, cx), self.find.mark(ix))
        };
        let theme = cx.theme();
        let (picked_bg, mark_bg) = (
            theme.primary.opacity(0.08),
            mark.map(|current| theme.warning.opacity(if current { 0.22 } else { 0.08 })),
        );
        let row = row
            .when_some(mark_bg, |d, bg| d.bg(bg))
            // Over the row, so the added and removed tints still show under it.
            .when(picked, |d| {
                d.child(div().absolute().inset_0().bg(picked_bg))
            })
            .children(unified_gutter)
            .when_some(over, |d, rows| {
                d.on_mouse_move(
                    cx.listener(move |this, e, window, cx| this.drag_over(rows, e, window, cx)),
                )
            });
        if notes.is_empty() {
            return row.into_any_element();
        }
        gpui::div()
            .w_full()
            .flex()
            .flex_col()
            .child(row)
            .children(notes)
            .into_any_element()
    }

    /// Row `ix` across the whole width: a file or hunk header, a placeholder, or a unified line.
    fn full_row(
        &self,
        base: gpui::Stateful<gpui::Div>,
        ix: usize,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = cx.theme();
        let Some(changes) = &self.changes else {
            return base;
        };
        match changes.rows.get(ix) {
            Some(Row::File(i)) => {
                let f = &changes.files[*i];
                let name = match &f.old_path {
                    Some(old) => format!("{} → {}", old.display(), f.path.display()),
                    None => f.path.display().to_string(),
                };
                // T3's file header: sans 12, medium, on the muted surface with a hairline.
                base.h(px(ROW_HEIGHT))
                    .items_center()
                    .px_3()
                    .gap_2()
                    .bg(theme.muted)
                    .border_t_1()
                    .border_color(theme.border)
                    .font_family(theme.font_family.clone())
                    .font_weight(FontWeight::MEDIUM)
                    .child(kind_badge(f.kind, cx))
                    .child(name)
            }
            Some(Row::Hunk(h)) => base
                .items_center()
                .px_3()
                .font_family(theme.font_family.clone())
                .text_size(px(11.))
                .text_color(theme.muted_foreground.opacity(0.7))
                .child(h.clone()),
            Some(Row::Placeholder(p)) => base
                .px_2()
                .italic()
                .text_color(theme.muted_foreground)
                .child(*p),
            Some(Row::TooLarge(n)) => base
                .px_2()
                .italic()
                .text_color(theme.muted_foreground)
                .child(format!(
                    "Diff too large: {n} lines. Open it in a terminal or editor."
                )),
            Some(Row::Line {
                old,
                new,
                kind,
                text,
            }) => line_parts(
                base,
                *old,
                Some(*new),
                *kind,
                text.clone().into(),
                self.prefs.wrap,
                theme,
            ),
            None => base,
        }
    }

    /// The file tree's visible rows: a folder click folds it, a file click scrolls the diff to it.
    pub(super) fn render_files(
        &mut self,
        range: Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = cx.theme();
        let Some(changes) = &self.changes else {
            return Vec::new();
        };
        range
            .filter_map(|ix| self.tree.get(ix).map(|r| (ix, r)))
            .map(|(ix, r)| {
                #[cfg(test)]
                FILE_ROWS_BUILT.with(|n| n.set(n.get() + 1));
                let depth = match r {
                    TreeRow::Dir { depth, .. } | TreeRow::File { depth, .. } => *depth,
                };
                let row = h_flex()
                    .id(("file", ix))
                    .w_full()
                    .h(px(FILE_ROW_HEIGHT))
                    .pl(px(8. + TREE_INDENT * depth as f32))
                    .pr_2()
                    .gap_1p5()
                    .text_size(ui::META)
                    .rounded(ui::RADIUS_MD)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.accent));
                match r {
                    TreeRow::Dir {
                        path, label, open, ..
                    } => {
                        let path = path.clone();
                        row.text_color(theme.muted_foreground)
                            .child(
                                Icon::new(if *open {
                                    SkyeIcon::ChevronDown
                                } else {
                                    SkyeIcon::ChevronRight
                                })
                                .xsmall(),
                            )
                            .child(div().truncate().child(label.clone()))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.toggle_folder(&path, cx)),
                            )
                    }
                    TreeRow::File {
                        ix: file, label, ..
                    } => {
                        let f = &changes.files[*file];
                        let (row_ix, path) = (f.first_row, f.path.clone());
                        let ticked = self.selected.contains(&f.path);
                        let active = self.active_file.as_ref() == Some(&f.path);
                        // Ticks pick files to commit: only the working tree commits.
                        let tick =
                            (self.scope == super::changes_model::Scope::Working).then(|| {
                                Checkbox::new(SharedString::from(format!(
                                    "pick-{}",
                                    f.path.display()
                                )))
                                .xsmall()
                                .checked(ticked)
                                .on_click(cx.listener({
                                    let path = path.clone();
                                    move |this, _, _, cx| {
                                        if !this.selected.remove(&path) {
                                            this.selected.insert(path.clone());
                                        }
                                        cx.notify();
                                    }
                                }))
                            });
                        row.when(active, |d| d.bg(theme.accent))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.scroll_to_row(row_ix);
                                this.active_file = Some(path.clone());
                                cx.notify();
                            }))
                            .children(tick)
                            .child(kind_badge(f.kind, cx))
                            .child(div().truncate().child(label.clone()))
                    }
                }
                .into_any_element()
            })
            .collect()
    }
}

#[cfg(test)]
thread_local! {
    static FILE_ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many file sidebar rows have been built on this thread, so tests can check only visible ones are.
#[cfg(test)]
pub(in crate::workspace) fn file_rows_built() -> usize {
    FILE_ROWS_BUILT.with(|n| n.get())
}

fn kind_badge(kind: ChangeKind, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (letter, color) = match kind {
        ChangeKind::Added => ("A", theme.success),
        ChangeKind::Deleted => ("D", theme.danger),
        ChangeKind::Renamed => ("R", theme.info),
        ChangeKind::Copied => ("C", theme.info),
        // T3 gives modified files a muted blue so added and deleted ones lead.
        ChangeKind::Modified | ChangeKind::TypeChanged => ("M", theme.info.opacity(0.75)),
        ChangeKind::Unmerged => ("U", theme.danger),
    };
    div()
        .w(px(12.))
        .flex_none()
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter)
}

/// One diff line on `base`: both line numbers, the sign, the text, tinted when added or removed. The Changes tab and
/// the timeline's edit cards draw lines with it.
pub(in crate::workspace) fn diff_line(
    base: gpui::Stateful<gpui::Div>,
    old: Option<u32>,
    new: Option<u32>,
    kind: LineKind,
    text: SharedString,
    theme: &gpui_component::Theme,
) -> gpui::Stateful<gpui::Div> {
    line_parts(base, old, Some(new), kind, text, false, theme)
}

/// A line's number column(s), sign and text on `base`; `second` is the new-side number in a unified row (`None`:
/// one column, a split half). `wrap` lets the text wrap under itself.
fn line_parts<E: ParentElement + Styled>(
    base: E,
    first: Option<u32>,
    second: Option<Option<u32>>,
    kind: LineKind,
    text: SharedString,
    wrap: bool,
    theme: &gpui_component::Theme,
) -> E {
    let num = |n: Option<u32>| {
        div()
            .w(px(NUM_W))
            .flex_none()
            .text_right()
            .pr_2()
            .text_color(theme.muted_foreground)
            .child(n.map(|n| n.to_string()).unwrap_or_default())
    };
    let (sign, bg): (&str, Option<Hsla>) = match kind {
        LineKind::Context => (" ", None),
        LineKind::Added => ("+", Some(theme.success.opacity(0.12))),
        LineKind::Removed => ("-", Some(theme.danger.opacity(0.12))),
    };
    let text = div()
        .flex_1()
        .min_w_0()
        .when(wrap, |d| d.whitespace_normal())
        .child(text);
    let base = match bg {
        Some(bg) => base.bg(bg),
        None => base,
    };
    base.child(num(first))
        .children(second.map(num))
        .child(div().w(px(SIGN_W)).flex_none().child(sign))
        .child(text)
}
