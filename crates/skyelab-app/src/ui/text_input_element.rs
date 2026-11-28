//! Draws the text input's one line: text or placeholder, selection, composition underline and caret, scrolled so the
//! caret shows. It never notifies from layout or paint.

use super::ActiveTheme as _;
use super::text_input::TextInput;
use gpui::{
    App, Bounds, DispatchPhase, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, LayoutId, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, ShapedLine,
    SharedString, Style, TextAlign, TextRun, UnderlineStyle, Window, fill, point, px, relative,
    size,
};
use std::ops::Range;

const CARET_WIDTH: Pixels = px(1.);
/// Kept free after the text when it scrolls, so the caret at its end has room.
const END_MARGIN: Pixels = px(10.);

/// The last drawn frame, for hit tests and the input method.
pub(super) struct Layout {
    /// The text it shows (empty while the placeholder shows).
    pub text: String,
    line: ShapedLine,
    /// The part of the window the text shows in.
    pub bounds: Bounds<Pixels>,
    scroll_x: Pixels,
    line_height: Pixels,
    pub caret: Option<Bounds<Pixels>>,
}

impl Layout {
    fn x(&self, offset: usize) -> Pixels {
        let offset = if self.text.is_empty() { 0 } else { offset };
        self.bounds.left() + self.scroll_x + self.line.x_for_index(offset)
    }

    pub(super) fn range_bounds(&self, range: &Range<usize>) -> Bounds<Pixels> {
        let top = self.bounds.top();
        Bounds::from_corners(
            point(self.x(range.start), top),
            point(self.x(range.end), top + self.line_height),
        )
    }

    pub(super) fn offset_at(&self, x: Pixels) -> usize {
        if self.text.is_empty() {
            return 0;
        }
        let x = x - self.bounds.left() - self.scroll_x;
        self.line.closest_index_for_x(x)
    }

    pub(super) fn clamp_scroll(&self, x: Pixels) -> Pixels {
        let width = if self.text.is_empty() {
            px(0.)
        } else {
            self.line.width() + END_MARGIN
        };
        x.clamp((self.bounds.size.width - width).min(px(0.)), px(0.))
    }

    /// The scroll that brings `offset` into view, from `x`.
    fn reveal(&self, offset: usize, x: Pixels) -> Pixels {
        let at = self.line.x_for_index(offset);
        let room = self.bounds.size.width - END_MARGIN;
        if x + at > room {
            room - at
        } else if x + at < px(0.) {
            -at
        } else {
            x
        }
    }
}

pub(super) struct TextElement {
    input: Entity<TextInput>,
}

impl TextElement {
    pub(super) fn new(input: Entity<TextInput>) -> Self {
        Self { input }
    }
}

impl Element for TextElement {
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
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
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
        let muted = cx.theme().muted_foreground;
        let style = window.text_style();
        let line_height = window.line_height();
        self.input.update(cx, |input, _| {
            let text = input.buffer.text().to_owned();
            let color = if text.is_empty() { muted } else { style.color };
            let color = if input.is_disabled() {
                color.opacity(0.5)
            } else {
                color
            };
            let shown: SharedString = if text.is_empty() {
                input.placeholder.clone()
            } else {
                text.clone().into()
            };
            let run = |len, underline: bool| TextRun {
                len,
                font: style.font(),
                color,
                background_color: None,
                underline: underline.then_some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(color),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let marked = input.buffer.marked().unwrap_or(0..0);
            let runs: Vec<_> = [
                run(marked.start, false),
                run(marked.len(), true),
                run(shown.len() - marked.end, false),
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let line = window
                .text_system()
                .shape_line(shown, font_size, &runs, None);
            let mut layout = Layout {
                text,
                line,
                bounds,
                scroll_x: px(0.),
                line_height,
                caret: None,
            };
            let selection = (input.buffer.anchor(), input.buffer.head());
            let all = input.buffer.range() == (0..layout.text.len());
            if input.revealed.replace(selection) != Some(selection) && !all {
                input.scroll_x = layout.reveal(selection.1, input.scroll_x);
            }
            input.scroll_x = layout.clamp_scroll(input.scroll_x);
            layout.scroll_x = input.scroll_x;
            if input.focus.is_focused(window) && !input.is_disabled() {
                let height = line_height * 0.85;
                let top = bounds.top() + (line_height - height) / 2.;
                let origin = point(layout.x(selection.1), top);
                layout.caret = Some(Bounds::new(origin, size(CARET_WIDTH, height)));
            }
            input.layout = Some(layout);
        });
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let (selection_color, caret_color) = (cx.theme().selection, cx.theme().caret);
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        self.on_drag(window);
        let input = self.input.read(cx);
        let Some(layout) = input.layout.as_ref() else {
            return;
        };
        let active = window.is_window_active();
        let selected = input.buffer.range();
        let shows_selection = focus.is_focused(window) || input.menu.is_some();
        if active && shows_selection && !selected.is_empty() && input.buffer.marked().is_none() {
            window.paint_quad(fill(layout.range_bounds(&selected), selection_color));
        }
        let caret = layout.caret.filter(|_| active && input.blink.visible);
        let origin = point(bounds.left() + layout.scroll_x, bounds.top());
        let (line, line_height) = (layout.line.clone(), layout.line_height);
        _ = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
        if let Some(mut caret) = caret {
            // On whole device pixels, or a 1 px caret smears over two.
            let scale = window.scale_factor();
            caret.origin.x = (caret.origin.x * scale).round() / scale;
            window.paint_quad(fill(caret, caret_color));
        }
    }
}

impl TextElement {
    /// A drag keeps selecting outside the field, so it listens to the window.
    fn on_drag(&self, window: &mut Window) {
        let input = self.input.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !input.read(cx).selecting {
                return;
            }
            input.update(cx, |input, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    input.drag_to(event.position, window, cx);
                } else {
                    input.selecting = false;
                }
            });
        });
        let input = self.input.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && input.read(cx).selecting {
                input.update(cx, |input, _| input.selecting = false);
            }
        });
    }
}
