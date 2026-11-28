//! The text of a field and every edit to it, with no GPUI types: offsets are bytes, the caret moves by grapheme
//! cluster, and undo steps are runs of one kind of edit.

use std::{borrow::Cow, ops::Range};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation as _};

/// The most one paste, accessibility `SetValue` or platform insert adds: a field shapes its whole line each frame.
pub(super) const MAX_INSERT: usize = 64 * 1024;
const MAX_STEPS: usize = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Intent {
    Typing,
    Backspace,
    DeleteForward,
    Compose,
    Atomic,
}

struct Step {
    start: usize,
    removed: String,
    inserted: String,
    /// (anchor, head) before and after.
    before: (usize, usize),
    after: (usize, usize),
    intent: Intent,
}

#[derive(Default)]
pub(super) struct Buffer {
    text: String,
    anchor: usize,
    head: usize,
    marked: Option<Range<usize>>,
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Whether the next edit may join the last step.
    joinable: bool,
}

/// `text` without what a single line can't hold.
pub(super) fn one_line(text: &str) -> Cow<'_, str> {
    if text.contains(['\n', '\r', '\t']) {
        Cow::Owned(text.replace(['\n', '\r', '\t'], ""))
    } else {
        Cow::Borrowed(text)
    }
}

/// The first [`MAX_INSERT`] bytes of `text`, cut between grapheme clusters (between characters when one cluster alone
/// is longer).
pub(super) fn clip(text: &str) -> &str {
    if text.len() <= MAX_INSERT {
        return text;
    }
    let at = text.floor_char_boundary(MAX_INSERT);
    let mut cursor = GraphemeCursor::new(at, text.len(), true);
    let cut = match cursor.is_boundary(text, 0) {
        Ok(false) => cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0),
        _ => at,
    };
    &text[..if cut == 0 { at } else { cut }]
}

/// Letters of scripts that put spaces between words (not Chinese or Japanese).
fn is_word(c: char) -> bool {
    let spaced = c < '\u{2E80}' || ('\u{AC00}'..='\u{D7A3}').contains(&c);
    c == '_' || (c.is_alphanumeric() && spaced) || ('\u{0300}'..='\u{036F}').contains(&c)
}

impl Buffer {
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn head(&self) -> usize {
        self.head
    }

    pub(super) fn anchor(&self) -> usize {
        self.anchor
    }

    pub(super) fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub(super) fn marked(&self) -> Option<Range<usize>> {
        self.marked.clone()
    }

    /// New text from the program: the caret at its end, nothing to undo.
    pub(super) fn set_text(&mut self, text: &str) {
        *self = Self {
            text: one_line(text).into_owned(),
            ..Self::default()
        };
        self.anchor = self.text.len();
        self.head = self.anchor;
    }

    pub(super) fn snap(&self, offset: usize) -> usize {
        let offset = self.text.floor_char_boundary(offset);
        match GraphemeCursor::new(offset, self.text.len(), true).is_boundary(&self.text, 0) {
            Ok(false) => self.prev_grapheme(offset),
            _ => offset,
        }
    }

    pub(super) fn prev_grapheme(&self, offset: usize) -> usize {
        GraphemeCursor::new(offset, self.text.len(), true)
            .prev_boundary(&self.text, 0)
            .ok()
            .flatten()
            .unwrap_or(0)
    }

    pub(super) fn next_grapheme(&self, offset: usize) -> usize {
        GraphemeCursor::new(offset, self.text.len(), true)
            .next_boundary(&self.text, 0)
            .ok()
            .flatten()
            .unwrap_or(self.text.len())
    }

    /// The start of the word before `offset`.
    pub(super) fn prev_word(&self, offset: usize) -> usize {
        self.text[..offset]
            .split_word_bound_indices()
            .rfind(|(_, s)| !s.trim().is_empty())
            .map_or(0, |(i, _)| i)
    }

    /// The end of the word after `offset`.
    pub(super) fn next_word(&self, offset: usize) -> usize {
        self.text[offset..]
            .split_word_bound_indices()
            .find(|(_, s)| !s.trim().is_empty())
            .map_or(self.text.len(), |(i, s)| offset + i + s.len())
    }

    /// The end of the run of spaces at `offset`, or `offset` when it isn't at one.
    pub(super) fn spaces_end(&self, offset: usize) -> usize {
        self.text.len() - self.text[offset..].trim_start().len()
    }

    /// What a double click at `offset` selects: a word, a run of spaces, or one other character.
    pub(super) fn word_at(&self, offset: usize) -> Range<usize> {
        let mut start = self.snap(offset);
        if start == self.text.len() {
            start = self.prev_grapheme(start);
        }
        let Some(c) = self.text[start..].chars().next() else {
            return 0..0;
        };
        let same: fn(char) -> bool = if is_word(c) {
            is_word
        } else if c.is_whitespace() {
            char::is_whitespace
        } else {
            return start..self.next_grapheme(start);
        };
        let before = self.text[..start].trim_end_matches(same).len();
        before..self.text.len() - self.text[start..].trim_start_matches(same).len()
    }

    pub(super) fn to_utf16(&self, offset: usize) -> usize {
        self.text[..offset].encode_utf16().count()
    }

    /// The bytes of a UTF-16 range, widened to whole characters and clamped to the text.
    pub(super) fn bytes_of_utf16(&self, range: &Range<usize>) -> Range<usize> {
        let (from, to) = (range.start.min(range.end), range.start.max(range.end));
        let (mut start, mut end) = (None, None);
        let mut units = 0;
        for (i, c) in self.text.char_indices() {
            let next = units + c.len_utf16();
            if start.is_none() && from < next {
                start = Some(i);
            }
            if end.is_none() && to <= units {
                end = Some(i);
            }
            units = next;
        }
        let len = self.text.len();
        let start = start.unwrap_or(len);
        if from == to {
            return start..start;
        }
        start..end.unwrap_or(len).max(start)
    }

    /// A caret move ends the undo step being typed.
    pub(super) fn select(&mut self, anchor: usize, head: usize) {
        (self.anchor, self.head) = (self.snap(anchor), self.snap(head));
        self.joinable = false;
    }

    pub(super) fn move_to(&mut self, offset: usize) {
        self.select(offset, offset);
    }

    pub(super) fn select_to(&mut self, offset: usize) {
        self.select(self.anchor, offset);
    }

    /// Ends the undo step being typed where the caret is.
    pub(super) fn end_step(&mut self) {
        self.joinable = false;
    }

    /// Replaces `range` (byte offsets on character boundaries) and puts the caret after `new`. False when that
    /// changes nothing.
    pub(super) fn replace(&mut self, range: Range<usize>, new: &str, intent: Intent) -> bool {
        self.marked = None;
        if self.text[range.clone()] == *new {
            self.select(range.end, range.end);
            return false;
        }
        let before = (self.anchor, self.head);
        let removed = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), new);
        self.head = range.start + new.len();
        self.anchor = self.head;
        self.redo.clear();
        let after = (self.anchor, self.head);
        let joined = self.joinable
            && self.undo.last_mut().is_some_and(|last| {
                if last.intent != intent {
                    return false;
                }
                let inserted = last.start..last.start + last.inserted.len();
                match intent {
                    Intent::Typing if removed.is_empty() && range.start == inserted.end => {
                        last.inserted.push_str(new);
                    }
                    Intent::Backspace if new.is_empty() && range.end == last.start => {
                        last.removed.insert_str(0, &removed);
                        last.start = range.start;
                    }
                    Intent::DeleteForward if new.is_empty() && range.start == last.start => {
                        last.removed.push_str(&removed);
                    }
                    Intent::Compose if range == inserted => last.inserted = new.to_owned(),
                    _ => return false,
                }
                last.after = after;
                true
            });
        if !joined {
            if self.undo.len() == MAX_STEPS {
                self.undo.remove(0);
            }
            self.undo.push(Step {
                start: range.start,
                removed,
                inserted: new.to_owned(),
                before,
                after,
                intent,
            });
        }
        self.joinable = intent != Intent::Atomic;
        true
    }

    /// One step of an IME composition: `new` replaces `range` and stays marked; `caret` is a byte range inside `new`.
    pub(super) fn compose(
        &mut self,
        range: Range<usize>,
        new: &str,
        caret: Option<Range<usize>>,
    ) -> bool {
        let continues = self.marked.as_ref() == Some(&range);
        self.joinable &= continues;
        let changed = self.replace(range.clone(), new, Intent::Compose);
        self.joinable = true;
        // A composition cancelled back to what it replaced is nothing to undo.
        if self
            .undo
            .pop_if(|s| s.intent == Intent::Compose && s.removed == s.inserted)
            .is_some()
        {
            self.joinable = false;
        }
        self.marked = (!new.is_empty()).then(|| range.start..range.start + new.len());
        if let Some(caret) = caret {
            self.anchor = range.start + new.floor_char_boundary(caret.start);
            self.head = range.start + new.floor_char_boundary(caret.end);
        }
        changed
    }

    /// Text the input method commits over the composition: the same undo step.
    pub(super) fn commit(&mut self, range: Range<usize>, new: &str) -> bool {
        let changed = self.compose(range, new, None);
        self.unmark();
        changed
    }

    pub(super) fn unmark(&mut self) {
        self.marked = None;
        self.joinable = false;
    }

    pub(super) fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
            return false;
        };
        let end = step.start + step.inserted.len();
        self.text.replace_range(step.start..end, &step.removed);
        (self.anchor, self.head) = step.before;
        self.unmark();
        self.redo.push(step);
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        let end = step.start + step.removed.len();
        self.text.replace_range(step.start..end, &step.inserted);
        (self.anchor, self.head) = step.after;
        self.unmark();
        self.undo.push(step);
        true
    }
}
