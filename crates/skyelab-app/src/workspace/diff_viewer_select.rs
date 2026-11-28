//! Joins `DiffViewer`'s virtualized rows to the window's text selection (`gpui_base::TextSelection`). An endpoint is a
//! content key, (line, byte of its drawn text), so a selection and its copy outlive the rows that scroll away.

use super::diff_view::ROW_H;
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, EntityId, GlobalElementId, Hitbox, HitboxBehavior,
    InspectorElementId, LayoutId, ListState, Pixels, Point, ScrollEvent, SharedString, StyledText,
    TextLayout, Window, px,
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

/// How many columns `c` takes at `column`: a tab reaches the next 4-column stop.
fn columns(c: char, column: usize) -> usize {
    if c == '\t' { TAB - column % TAB } else { 1 }
}

/// `text` as drawn: each tab becomes spaces to its stop.
pub(super) fn expand_tabs(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        let width = columns(c, column);
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
        let width = columns(c, column);
        let bytes = if c == '\t' { width } else { c.len_utf8() };
        if drawn < at + bytes {
            return byte;
        }
        column += width;
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

struct Run {
    line: usize,
    text: SharedString,
    layout: TextLayout,
    visible: Bounds<Pixels>,
}

/// Where the last frame drew each row's code.
#[derive(Default)]
pub(super) struct Geometry {
    origin: Point<Pixels>,
    scroll: Point<Pixels>,
    runs: Vec<Run>,
    hinted_width: Option<Pixels>,
    /// How far a Page Up (or a big upward wheel turn) still has to go: it moves at most [`UP_STEP`] a frame, over rows
    /// the list has measured, or it would jump over wrapped rows it only knew by their one-row hint.
    pub(super) pending_up: Pixels,
    /// Where the current drag's end was last over our text, for a drag that ends outside the viewer.
    pub(super) last_cursor: Option<u64>,
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

    /// The key of the character boundary nearest `point`, in window coordinates.
    pub(super) fn key_at_window(&self, at: Point<Pixels>) -> Option<u64> {
        let run = self.run_at(at)?;
        let layout = run.layout.line_layouts();
        let byte = match layout.first() {
            Some(line) => line
                .closest_index_for_position(
                    at - run.layout.bounds().origin,
                    run.layout.line_height(),
                )
                .unwrap_or_else(|ix| ix),
            None => 0,
        };
        Some(key(run.line, byte.min(run.text.len())))
    }

    /// The key for `point`, relative to the content, as the window selection asks.
    pub(super) fn key_at(&self, point: Point<Pixels>) -> Option<u64> {
        self.key_at_window(point + self.origin + self.scroll)
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
    pub(super) hitbox: Option<Hitbox>,
}

/// The most a Page Up moves in one frame: within the rows the list lays out above the view ([`OVERDRAW`]).
pub(super) const UP_STEP: Pixels = px(320.);
/// How far above and below the view the list lays rows out.
pub(super) const OVERDRAW: Pixels = px(400.);

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
        self.hitbox = Some(hitbox.clone());
        self.child.prepaint(window, cx);
        // GPUI's list forgets every row height, hints too, at a new width (its first layout included), and the wheel
        // and the scrollbar then stop at the last row measured.
        let width = self.list.viewport_bounds().size.width;
        if width > px(0.) && self.geometry.borrow().hinted_width != Some(width) {
            self.geometry.borrow_mut().hinted_width = Some(width);
            let _ = self.list.clone().with_uniform_item_height(self.row_height);
            cx.notify(self.view);
        }
        let pending = self.geometry.borrow().pending_up;
        if pending > px(0.) {
            let (list, geometry, view) = (self.list.clone(), self.geometry.clone(), self.view);
            cx.defer(move |cx| {
                let step = pending.min(UP_STEP);
                list.scroll_by(-step);
                geometry.borrow_mut().pending_up = pending - step;
                cx.notify(view);
            });
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
        let (geometry, view, hitbox) = (self.geometry.clone(), self.view, self.hitbox.take());
        window.on_mouse_event(move |event: &ScrollEvent, phase, _, cx| {
            let up = event.delta.pixel_delta(px(ROW_H)).y;
            let ours = hitbox
                .as_ref()
                .is_some_and(|h| h.bounds.contains(&event.position));
            if phase.capture() && ours && up > UP_STEP {
                geometry.borrow_mut().pending_up += up;
                cx.stop_propagation();
                cx.notify(view);
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
}
