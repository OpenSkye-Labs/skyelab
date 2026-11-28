//! A whole change to read before allowing it (an approval's View all): the approval card's rows, every line of them,
//! only those in view built. Text selects across rows and copies as code alone; the keyboard, the wheel and the
//! scrollbar reach every line.

use super::changes_model::LineKind;
use super::diff_view::{self, Line, ROW_H};
use super::diff_viewer_select::{
    Code, Geometry, Layer, expand_tabs, key, lines_between, selected_on, source_byte,
};
use crate::ui::syntax::{self, Spans};
use crate::ui::{ActiveTheme as _, v_flex};
use gpui::{
    AnyElement, App, ClipboardItem, Context, FocusHandle, Focusable, HighlightStyle, IntoElement,
    KeyBinding, ListAlignment, ListOffset, ListState, Pixels, Render, SharedString, StyledText,
    Subscription, Task, Window, accesskit, combine_highlights, div, list, prelude::*, px,
};
use gpui_base::{Scrollbar, TextSelection, TextSelectionContentKey, TextSelectionHandle};
use std::{cell::RefCell, rc::Rc};

gpui::actions!(
    skye_diff,
    [
        ScrollUp,
        ScrollDown,
        PageUp,
        PageDown,
        First,
        Last,
        SelectAll,
        CopySelection
    ]
);

const KEY_CONTEXT: &str = "SkyeDiff";
/// Longer lines stay plain: a minified file would hold the parser for seconds.
const MAX_PARSED_LINE: usize = 1_000;

pub(super) fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    let (all, copy) = if cfg!(target_os = "macos") {
        ("cmd-a", "cmd-c")
    } else {
        ("ctrl-a", "ctrl-c")
    };
    cx.bind_keys([
        KeyBinding::new("up", ScrollUp, ctx),
        KeyBinding::new("down", ScrollDown, ctx),
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
        KeyBinding::new("home", First, ctx),
        KeyBinding::new("end", Last, ctx),
        KeyBinding::new(all, SelectAll, ctx),
        KeyBinding::new(copy, CopySelection, ctx),
    ]);
}

struct Row {
    number: Option<u32>,
    kind: LineKind,
    source: SharedString,
    /// `source` with its tabs expanded.
    drawn: SharedString,
}

pub(super) struct DiffViewer {
    path: SharedString,
    rows: Rc<Vec<Row>>,
    /// The rows a MultiEdit's later edits start at.
    breaks: Rc<Vec<usize>>,
    spans: Rc<Vec<Spans>>,
    gutter: Pixels,
    list: ListState,
    focus: FocusHandle,
    selection: TextSelectionHandle,
    geometry: Rc<RefCell<Geometry>>,
    _colors: Task<()>,
    _selection_changed: Subscription,
}

#[cfg(test)]
static BYTES_PARSED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many bytes of code any viewer has handed to the syntax parser.
#[cfg(test)]
pub(super) fn bytes_parsed() -> usize {
    BYTES_PARSED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Each edit's spans, parsed on its own: a comment one edit leaves open doesn't color the next.
fn colors(lines: &[(LineKind, SharedString)], breaks: &[usize], language: &str) -> Vec<Spans> {
    let ends = breaks.iter().copied().chain([lines.len()]);
    let mut start = 0;
    let mut out = Vec::with_capacity(lines.len());
    for end in ends {
        let edit = lines[start..end].iter().map(|(kind, text)| {
            let parsed = if text.len() > MAX_PARSED_LINE {
                ""
            } else {
                text
            };
            (*kind, parsed)
        });
        #[cfg(test)]
        BYTES_PARSED.fetch_add(
            edit.clone().map(|l| l.1.len()).sum(),
            std::sync::atomic::Ordering::SeqCst,
        );
        out.extend(diff_view::spans(edit, language));
        start = end;
    }
    out
}

/// The selection's two ends in reading order; everything after select-all.
fn selected_keys(selection: &TextSelectionHandle, rows: &[Row], cx: &App) -> Option<(u64, u64)> {
    if selection.has_local_selection(cx) {
        let last = rows.len().checked_sub(1)?;
        return Some((0, key(last, rows[last].drawn.len())));
    }
    let snapshot = selection.snapshot(cx)?;
    let ours = Some(selection.entity_id());
    let end = |end: gpui_base::TextSelectionEndpoint| {
        (end.entity_id() == ours)
            .then(|| end.content_key())
            .flatten()
    };
    let (a, b) = (
        end(snapshot.anchor())?.value(),
        end(snapshot.cursor())?.value(),
    );
    (a != b).then_some((a.min(b), a.max(b)))
}

fn selected_text(selection: &TextSelectionHandle, rows: &[Row], cx: &App) -> String {
    let Some(keys) = selected_keys(selection, rows, cx) else {
        return String::new();
    };
    let parts: Vec<&str> = lines_between(keys)
        .filter_map(|ix| {
            let row = rows.get(ix)?;
            let drawn = selected_on(ix, row.drawn.len(), keys)?;
            let source = source_byte(&row.source, drawn.start)..source_byte(&row.source, drawn.end);
            row.source.get(source)
        })
        .collect();
    parts.join("\n")
}

impl DiffViewer {
    /// `breaks`: ascending indexes into `lines` where a MultiEdit's later edits start.
    pub(super) fn new(
        path: &str,
        lines: Vec<Line>,
        breaks: Vec<usize>,
        cx: &mut Context<Self>,
    ) -> Self {
        let rows: Rc<Vec<Row>> = Rc::new(
            lines
                .into_iter()
                .map(|(old, new, kind, text)| Row {
                    number: new.or(old),
                    kind,
                    drawn: expand_tabs(&text).into(),
                    source: text.into(),
                })
                .collect(),
        );
        let widest = rows.iter().filter_map(|r| r.number).max().unwrap_or(1);
        let focus = cx.focus_handle().tab_stop(true);
        let geometry = Rc::new(RefCell::new(Geometry::default()));
        let selection = TextSelectionHandle::new("", cx);
        let resolve = geometry.clone();
        selection.resolve_content_key_with(
            move |point, _| {
                let key = resolve.borrow_mut().key_at(point)?;
                Some(TextSelectionContentKey::new(key))
            },
            cx,
        );
        let (copied, copied_rows) = (selection.clone(), rows.clone());
        selection.copy_with(move |cx| selected_text(&copied, &copied_rows, cx), cx);
        // Copy is dispatched from the focus, so a drag has to bring it here.
        let focused = focus.clone();
        selection.focus_with(move |window, cx| window.focus(&focused, cx), cx);
        let view = cx.weak_entity();
        let changed = selection.subscribe(
            move |_, cx| {
                let _ = view.update(cx, |_, cx| cx.notify());
            },
            cx,
        );
        let parse: Vec<_> = rows.iter().map(|r| (r.kind, r.drawn.clone())).collect();
        let language = syntax::language(std::path::Path::new(path));
        let edits = breaks.clone();
        let parsed = cx.background_spawn(async move { colors(&parse, &edits, &language) });
        let colors = cx.spawn(async move |this, cx| {
            let spans = parsed.await;
            let _ = this.update(cx, |this, cx| {
                this.spans = Rc::new(spans);
                cx.notify();
            });
        });
        Self {
            path: path.to_owned().into(),
            spans: Rc::new(vec![Spans::new(); rows.len()]),
            gutter: diff_view::gutter_width(widest),
            list: ListState::new(rows.len(), ListAlignment::Top, px(ROW_H * 4.)),
            rows,
            breaks: Rc::new(breaks),
            focus,
            selection,
            geometry,
            _colors: colors,
            _selection_changed: changed,
        }
    }

    fn scroll(&self, by: Pixels, cx: &mut Context<Self>) {
        self.list.scroll_by(by);
        cx.notify();
    }

    /// A page keeps the last whole row in view, under a row that may be cut.
    fn page(&self) -> Pixels {
        (self.list.viewport_bounds().size.height - px(2. * ROW_H)).max(px(ROW_H))
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        TextSelection::clear(window, cx);
        self.selection.set_local_selection(true, cx);
        cx.notify();
    }

    /// Our own, as the window root's copy trims the text and with it a first line's indent.
    fn copy(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        let text = selected_text(&self.selection, &self.rows, cx);
        if text.is_empty() {
            cx.propagate();
        } else {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
}

impl Focusable for DiffViewer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DiffViewer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = Frame {
            rows: self.rows.clone(),
            breaks: self.breaks.clone(),
            spans: self.spans.clone(),
            selected: selected_keys(&self.selection, &self.rows, cx),
            gutter: self.gutter,
            geometry: self.geometry.clone(),
        };
        let drawn_rows = list(self.list.clone(), move |ix, _, cx| frame.row(ix, cx)).size_full();
        let theme = cx.theme();
        div()
            .id("diff-viewer")
            .relative()
            .size_full()
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .font_family(theme.mono_font_family.clone())
            .text_size(theme.mono_font_size)
            .role(accesskit::Role::Document)
            .aria_label(format!("Diff for {}", self.path))
            .map(gpui_base::TestSupportExt::test_support)
            .track_focus(&self.focus)
            .key_context(KEY_CONTEXT)
            .on_action(cx.listener(|this, _: &ScrollUp, _, cx| this.scroll(px(-2. * ROW_H), cx)))
            .on_action(cx.listener(|this, _: &ScrollDown, _, cx| this.scroll(px(2. * ROW_H), cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.scroll(-this.page(), cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.scroll(this.page(), cx)))
            .on_action(cx.listener(|this, _: &First, _, cx| {
                this.list.scroll_to(ListOffset::default());
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Last, _, cx| {
                this.list.scroll_to_end();
                cx.notify();
            }))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::copy))
            .child(Layer {
                child: drawn_rows.into_any_element(),
                list: self.list.clone(),
                row_height: px(ROW_H),
                selection: self.selection.clone(),
                geometry: self.geometry.clone(),
                view: cx.entity_id(),
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(Scrollbar::vertical(&self.list).viewport_from_layout()),
            )
    }
}

/// What one frame's rows are built from.
struct Frame {
    rows: Rc<Vec<Row>>,
    breaks: Rc<Vec<usize>>,
    spans: Rc<Vec<Spans>>,
    selected: Option<(u64, u64)>,
    gutter: Pixels,
    geometry: Rc<RefCell<Geometry>>,
}

impl Frame {
    fn row(&self, ix: usize, cx: &App) -> AnyElement {
        #[cfg(test)]
        diff_view::VIEWER_ROWS_BUILT.with(|n| n.set(n.get() + 1));
        let theme = cx.theme();
        let row = &self.rows[ix];
        let styles = &theme.highlight_theme;
        let syntax: Vec<_> = self.spans[ix]
            .iter()
            .filter_map(|(r, name)| Some((r.clone(), styles.style(name)?)))
            .collect();
        #[cfg(test)]
        self.geometry.borrow_mut().colored.insert(ix, syntax.len());
        let selected = self
            .selected
            .and_then(|keys| selected_on(ix, row.drawn.len(), keys))
            .map(|range| {
                let style = HighlightStyle {
                    background_color: Some(theme.selection),
                    ..Default::default()
                };
                (range, style)
            });
        let code = Code {
            line: ix,
            text: row.drawn.clone(),
            styled: StyledText::new(row.drawn.clone())
                .with_highlights(combine_highlights(syntax, selected)),
            geometry: self.geometry.clone(),
        };
        let number = row.number.unwrap_or_default();
        let what = match row.kind {
            LineKind::Context => format!("Line {number}"),
            LineKind::Added => format!("Added line {number}"),
            LineKind::Removed => format!("Removed line {number}"),
        };
        v_flex()
            .w_full()
            .when(self.breaks.binary_search(&ix).is_ok(), |d| {
                d.child(
                    div()
                        .debug_selector(move || format!("diff-viewer-break-{ix}"))
                        .h(px(1.))
                        .w_full()
                        .bg(theme.border),
                )
            })
            .child(
                diff_view::row_frame(row.number, row.kind, self.gutter, theme)
                    .id(("diff-viewer-row", ix))
                    .role(accesskit::Role::Label)
                    .aria_label(row.source.clone())
                    .aria_description(what)
                    .map(gpui_base::TestSupportExt::test_support)
                    .child(
                        div()
                            .debug_selector(move || format!("diff-viewer-code-{ix}"))
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .min_h(px(ROW_H))
                            .text_color(theme.foreground)
                            .whitespace_normal()
                            .child(
                                div()
                                    .debug_selector(move || format!("diff-viewer-text-{ix}"))
                                    .min_w_0()
                                    .child(code),
                            ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
impl DiffViewer {
    pub(super) fn list(&self) -> &ListState {
        &self.list
    }

    pub(super) fn drawn_text(&self, ix: usize) -> String {
        self.rows[ix].drawn.to_string()
    }

    pub(super) fn span_names(&self, ix: usize) -> Vec<&'static str> {
        self.spans[ix].iter().map(|s| s.1).collect()
    }

    /// How many syntax-colored runs each line was last drawn with.
    pub(super) fn painted_syntax_runs(&self) -> Vec<usize> {
        self.geometry.borrow().colored.values().copied().collect()
    }
}
