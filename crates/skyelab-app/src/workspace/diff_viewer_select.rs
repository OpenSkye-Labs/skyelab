//! Joins `DiffViewer`'s virtualized rows to the window's text selection (`gpui_base::TextSelection`). An endpoint is a
//! content key, (line, byte of its drawn text), so a selection and its copy outlive the rows that scroll away.

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, HitboxBehavior,
    InspectorElementId, LayoutId, ListState, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, SharedString, StyledText, TextLayout, Window, px,
};
use gpui_base::{TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};
use std::{cell::RefCell, ops::Range, rc::Rc};

const TAB: usize = 4;

pub(super) fn key(line: usize, byte: usize) -> u64 {
    ((line as u64) << 32) | byte as u64
}

fn line_and_byte(key: u64) -> (usize, usize) {
    ((key >> 32) as usize, (key & 0xffff_ffff) as usize)
}

/// `text` as drawn: each tab reaches the next 4-column stop.
pub(super) fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        let width = if c == '\t' { TAB - column % TAB } else { 1 };
        if c == '\t' {
            out.extend(std::iter::repeat_n(' ', width));
        } else {
            out.push(c);
        }
        column += width;
    }
    out
}

/// The byte of `text` that byte `drawn` of its [`expand_tabs`] comes from; inside a tab's spaces, the tab itself.
pub(super) fn source_byte(text: &str, drawn: usize) -> usize {
    let (mut column, mut at) = (0, 0);
    for (byte, c) in text.char_indices() {
        let (columns, bytes) = match c {
            '\t' => (TAB - column % TAB, TAB - column % TAB),
            c => (1, c.len_utf8()),
        };
        if drawn < at + bytes {
            return byte;
        }
        column += columns;
        at += bytes;
    }
    text.len()
}

/// The part of line `ix` (`len` bytes as drawn) between the keys `from` and `to`.
pub(super) fn selected_on(ix: usize, len: usize, (from, to): (u64, u64)) -> Option<Range<usize>> {
    let ((first, start), (last, end)) = (line_and_byte(from), line_and_byte(to));
    if ix < first || ix > last {
        return None;
    }
    let start = if ix == first { start.min(len) } else { 0 };
    let end = if ix == last { end.min(len) } else { len };
    Some(start..end.max(start))
}

/// The lines two keys span, first to last.
pub(super) fn lines_between((from, to): (u64, u64)) -> std::ops::RangeInclusive<usize> {
    line_and_byte(from).0..=line_and_byte(to).0
}

/// The word, run of spaces or single other character at `byte` of `text`.
fn word_at(text: &str, byte: usize) -> Range<usize> {
    let class = |c: char| match c {
        c if c == '_' || c.is_alphanumeric() => 1,
        c if c.is_whitespace() => 2,
        _ => 0,
    };
    let Some(c) = text.get(byte..).and_then(|t| t.chars().next()) else {
        return text.len()..text.len();
    };
    let same = |other: &char| class(c) != 0 && class(*other) == class(c);
    let len = |chars: &mut dyn Iterator<Item = char>| chars.map(char::len_utf8).sum::<usize>();
    let start = byte - len(&mut text[..byte].chars().rev().take_while(same));
    let end = byte + c.len_utf8();
    start..end + len(&mut text[end..].chars().take_while(same))
}

struct Run {
    line: usize,
    text: SharedString,
    layout: TextLayout,
    visible: Bounds<Pixels>,
}

/// A double or triple click: which, where, and whether its first end was already resolved.
type Multi = (usize, Point<Pixels>, bool);

/// Where the last frame drew each row's code.
#[derive(Default)]
pub(super) struct Geometry {
    origin: Point<Pixels>,
    scroll: Point<Pixels>,
    runs: Vec<Run>,
    multi: Option<Multi>,
    hinted_width: Option<Pixels>,
    #[cfg(test)]
    pub(super) colored: std::collections::BTreeMap<usize, usize>,
}

impl Geometry {
    fn run_at(&self, at: Point<Pixels>) -> Option<&Run> {
        self.runs.iter().min_by_key(|run| {
            let outside = at.y < run.visible.top() || at.y >= run.visible.bottom();
            let distance = f32::from((at.y - run.visible.center().y).abs());
            (outside, distance.to_bits())
        })
    }

    /// The key of the character at `point` (relative to the content, as the window selection asks). A double or triple
    /// click resolves its two ends in turn: the word's or the line's start, then its end.
    pub(super) fn key_at(&mut self, point: Point<Pixels>) -> Option<u64> {
        if let Some((count, at, started)) = self.multi {
            let run = self.run_at(at)?;
            let byte = run.layout.index_for_position(at).unwrap_or_else(|ix| ix);
            let range = if count >= 3 {
                0..run.text.len()
            } else {
                word_at(&run.text, byte)
            };
            let found = key(run.line, if started { range.end } else { range.start });
            self.multi = (!started).then_some((count, at, true));
            return Some(found);
        }
        let at = point + self.origin + self.scroll;
        let run = self.run_at(at)?;
        let byte = run.layout.index_for_position(at).unwrap_or_else(|ix| ix);
        Some(key(run.line, byte))
    }
}

/// One row's code, which tells the [`Geometry`] where it was laid out.
pub(super) struct Code {
    pub(super) line: usize,
    pub(super) text: SharedString,
    pub(super) styled: StyledText,
    pub(super) geometry: Rc<RefCell<Geometry>>,
}

impl Element for Code {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.styled.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.styled
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        self.geometry.borrow_mut().runs.push(Run {
            line: self.line,
            text: self.text.clone(),
            layout: self.styled.layout().clone(),
            visible: bounds.intersect(&window.content_mask().bounds),
        });
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.styled
            .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
    }
}

/// The list of rows, registered each frame as one participant of the window's selection.
pub(super) struct Layer {
    pub(super) child: AnyElement,
    pub(super) list: ListState,
    pub(super) row_height: Pixels,
    pub(super) selection: TextSelectionHandle,
    pub(super) geometry: Rc<RefCell<Geometry>>,
    pub(super) view: EntityId,
}

impl Element for Layer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("diff-viewer-selection".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.geometry.borrow_mut().runs.clear();
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        self.child.prepaint(window, cx);
        // GPUI's list forgets every row height, hints too, at a new width (its first layout included), and the wheel
        // and the scrollbar then stop at the last row measured.
        let width = self.list.viewport_bounds().size.width;
        if width > px(0.) && self.geometry.borrow().hinted_width != Some(width) {
            self.geometry.borrow_mut().hinted_width = Some(width);
            let _ = self.list.clone().with_uniform_item_height(self.row_height);
            cx.notify(self.view);
        }
        let scroll = self.list.scroll_px_offset_for_scrollbar();
        let text_bounds = {
            let mut geometry = self.geometry.borrow_mut();
            geometry.origin = bounds.origin;
            geometry.scroll = scroll;
            geometry.runs.iter().map(|run| run.visible).collect()
        };
        let registration = TextSelectionRegistration::new(hitbox, bounds)
            .with_scroll_offset(scroll)
            .with_text_bounds(text_bounds)
            .with_rendered_element(&self.selection, window, cx);
        self.selection.register(registration, window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let runs: Vec<_> = self
            .geometry
            .borrow()
            .runs
            .iter()
            .map(|run| {
                TextSelectionRun::new(run.text.clone(), run.layout.clone(), run.visible)
                    .with_document_order(key(run.line, 0))
            })
            .collect();
        self.selection.update_runs(&runs, cx);
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |event: &MouseDownEvent, phase, _, _| {
            if phase.capture() && event.button == MouseButton::Left {
                geometry.borrow_mut().multi =
                    (event.click_count >= 2).then_some((event.click_count, event.position, false));
            }
        });
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, _, _| {
            if phase.capture() {
                geometry.borrow_mut().multi = None;
            }
        });
        let geometry = self.geometry.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, _| {
            if phase.capture() {
                geometry.borrow_mut().multi = None;
            }
        });
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawn_bytes_map_back_past_tabs_and_wide_characters() {
        let text = "\té\tx";
        assert_eq!(expand_tabs(text), "    é   x");
        let map: Vec<usize> = [0, 3, 4, 6, 8, 9, 10, 99]
            .map(|drawn| source_byte(text, drawn))
            .into();
        assert_eq!(map, [0, 0, 1, 3, 3, 4, 5, 5]);
    }

    #[test]
    fn a_word_stops_at_spaces_and_punctuation() {
        let word = |byte| &"let a_é1 = f();"[word_at("let a_é1 = f();", byte)];
        assert_eq!(
            (word(0), word(5), word(3), word(13)),
            ("let", "a_é1", " ", "(")
        );
        assert_eq!(word_at("ab", 2), 2..2);
    }
}
