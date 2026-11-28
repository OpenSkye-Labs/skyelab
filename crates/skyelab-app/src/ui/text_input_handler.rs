//! The platform's side of the text input: typed text and input method composition, in UTF-16 offsets.

use super::text_edit::{Intent, bytes_of_utf16, fit};
use super::text_input::TextInput;
use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window};
use std::ops::Range;

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.buffer.bytes_of_utf16(&range);
        *adjusted_range = Some(self.utf16(&range));
        Some(self.buffer.text()[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        (ignore_disabled_input || !self.is_disabled()).then(|| UTF16Selection {
            range: self.utf16(&self.buffer.range()),
            reversed: self.buffer.head() < self.buffer.anchor(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer.marked().map(|r| self.utf16(&r))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_composition(window, cx);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_disabled() {
            return;
        }
        let range = self.target(range);
        if self.buffer.marked().is_some() {
            self.buffer.commit(range, &fit(text));
            self.end_composition(window, cx);
        } else {
            self.edit(range, text, Intent::Typing, window, cx);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_disabled() {
            return;
        }
        let range = self.target(range);
        let new = fit(new_text);
        let caret = new_selected_range.map(|r| bytes_of_utf16(&new, &r));
        if self.composing_from.is_none() {
            self.composing_from = Some(self.buffer.text().to_owned());
        }
        if self.buffer.compose(range, &new, caret) {
            window.notify_text_input_changed(&self.focus);
        }
        self.moved(window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        // Asked right after an edit, before the next frame: the last frame's line is the best place known.
        let layout = self.layout.as_ref()?;
        Some(layout.range_bounds(&bytes_of_utf16(&layout.text, &range_utf16)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let layout = self.layout.as_ref()?;
        (layout.text == self.buffer.text() && layout.bounds.contains(&point)).then(|| {
            self.buffer
                .to_utf16(self.buffer.snap(layout.offset_at(point.x)))
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.buffer.bytes_of_utf16(&range_utf16);
        self.buffer.select(range.start, range.end);
        self.moved(window, cx);
    }

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.is_disabled()
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.buffer.to_utf16(self.buffer.text().len()))
    }
}

impl TextInput {
    fn utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.buffer.to_utf16(range.start)..self.buffer.to_utf16(range.end)
    }

    fn target(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|r| self.buffer.bytes_of_utf16(&r))
            .or_else(|| self.buffer.marked())
            .unwrap_or_else(|| self.buffer.range())
    }
}
