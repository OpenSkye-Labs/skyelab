//! The platform's side of the text input: typed text and input method composition, in UTF-16 offsets.

use super::text_edit::{Intent, clip, one_line};
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

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.unmark();
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
            let text = one_line(text);
            let changed = self.buffer.commit(range, clip(&text));
            self.edited(changed, window, cx);
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
        let new = one_line(new_text);
        let new = clip(&new);
        let caret =
            new_selected_range.map(|r| utf16_to_bytes(new, r.start)..utf16_to_bytes(new, r.end));
        let changed = self.buffer.compose(range, new, caret);
        self.edited(changed, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.range_bounds(self.buffer.bytes_of_utf16(&range_utf16))
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

    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        !self.is_disabled()
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.buffer.to_utf16(self.buffer.text().len()))
    }
}

/// The byte offset of UTF-16 unit `at` in `text`, at or after it.
fn utf16_to_bytes(text: &str, at: usize) -> usize {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units >= at {
            return i;
        }
        units += c.len_utf16();
    }
    text.len()
}

impl TextInput {
    fn utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.buffer.to_utf16(range.start)..self.buffer.to_utf16(range.end)
    }

    /// What the platform replaces: the range it names, else the composition, else the selection.
    fn target(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        range_utf16
            .map(|r| self.buffer.bytes_of_utf16(&r))
            .or_else(|| self.buffer.marked())
            .unwrap_or_else(|| self.buffer.range())
    }
}
