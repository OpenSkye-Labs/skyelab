//! The text input's text as AccessKit text runs: a screen reader reads the characters, the caret and the selection
//! only from a field's `Role::TextRun` children. A character is a grapheme cluster, as the caret moves.

use super::text_input::TextInput;
use gpui::{A11ySubtreeBuilder, Bounds, Pixels, accesskit};
use unicode_segmentation::UnicodeSegmentation as _;

/// AccessKit holds a run's character lengths and word starts in `u8`s.
const RUN: usize = u8::MAX as usize;

struct Character {
    text: String,
    word_start: bool,
    bounds: Option<Bounds<Pixels>>,
}

pub(super) struct Runs {
    characters: Vec<Character>,
    /// (anchor, focus) in characters.
    selection: (usize, usize),
    scale: f32,
}

fn character_index(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .take_while(|(start, _)| *start < offset)
        .count()
}

fn capture(
    text: &str,
    (anchor, focus): (usize, usize),
    scale: f32,
    bounds_of: impl Fn(std::ops::Range<usize>) -> Option<Bounds<Pixels>>,
) -> Runs {
    let mut after_space = true;
    let characters = text
        .grapheme_indices(true)
        .map(|(at, g)| {
            let space = g.trim().is_empty();
            let word_start = std::mem::replace(&mut after_space, space) && !space && at != 0;
            Character {
                // A cluster too long for AccessKit's length byte reads as its first character.
                text: match g.len() > RUN {
                    true => g.chars().take(1).collect(),
                    false => g.to_owned(),
                },
                word_start,
                bounds: bounds_of(at..at + g.len()),
            }
        })
        .collect();
    Runs {
        characters,
        selection: (character_index(text, anchor), character_index(text, focus)),
        scale,
    }
}

pub(super) fn runs_of(input: &TextInput, scale: f32) -> Runs {
    let selection = (input.buffer.anchor(), input.buffer.head());
    capture(input.buffer.text(), selection, scale, |r| {
        input.range_bounds(r)
    })
}

impl Runs {
    /// (run, index in it) of character `at`: the end of a run stays in that run.
    fn position(&self, ids: &[accesskit::NodeId], at: usize) -> accesskit::TextPosition {
        let run = (at.saturating_sub(1) / RUN).min(ids.len() - 1);
        accesskit::TextPosition {
            node: ids[run],
            character_index: at - run * RUN,
        }
    }

    pub(super) fn push(&self, builder: &mut A11ySubtreeBuilder) {
        // An empty field still has one run, to hold the caret.
        let count = self.characters.len().div_ceil(RUN).max(1);
        let ids: Vec<_> = (0..count)
            .map(|i| builder.synthetic_node_id(("text-run", i)))
            .collect();
        for (i, &id) in ids.iter().enumerate() {
            let chunk = self.characters.chunks(RUN).nth(i).unwrap_or_default();
            let mut node = accesskit::Node::new(accesskit::Role::TextRun);
            if let Some(&next) = ids.get(i + 1) {
                node.set_next_on_line(next);
            }
            if i > 0 {
                node.set_previous_on_line(ids[i - 1]);
            }
            node.set_value(chunk.iter().map(|c| c.text.as_str()).collect::<String>());
            node.set_character_lengths(
                chunk.iter().map(|c| c.text.len() as u8).collect::<Vec<_>>(),
            );
            let starts = chunk.iter().enumerate().filter(|(_, c)| c.word_start);
            node.set_word_starts(starts.map(|(i, _)| i as u8).collect::<Vec<_>>());
            let bounds: Option<Vec<_>> = chunk.iter().map(|c| c.bounds).collect();
            if let Some(bounds) = bounds
                && let Some(all) = bounds.iter().copied().reduce(|a, b| a.union(&b))
            {
                let px = |p: Pixels| f32::from(p) * self.scale;
                let left = px(all.left());
                node.set_character_positions(
                    bounds
                        .iter()
                        .map(|b| px(b.left()) - left)
                        .collect::<Vec<_>>(),
                );
                node.set_character_widths(
                    bounds.iter().map(|b| px(b.size.width)).collect::<Vec<_>>(),
                );
                node.set_bounds(accesskit::Rect {
                    x0: left.into(),
                    y0: px(all.top()).into(),
                    x1: px(all.right()).into(),
                    y1: px(all.bottom()).into(),
                });
            }
            builder.push_child(id, node);
        }
        let (anchor, focus) = self.selection;
        builder
            .parent_node()
            .set_text_selection(accesskit::TextSelection {
                anchor: self.position(&ids, anchor),
                focus: self.position(&ids, focus),
            });
    }
}

#[cfg(test)]
pub(super) fn runs(text: &str, anchor: usize, focus: usize) -> Runs {
    capture(text, (anchor, focus), 1., |_| None)
}

#[cfg(test)]
impl Runs {
    pub(super) fn characters(&self) -> usize {
        self.characters.len()
    }

    pub(super) fn selection(&self) -> (usize, usize) {
        self.selection
    }
}
