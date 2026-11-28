//! The text input's keys: each is an action bound in the `SkyeTextInput` context.

use super::text_edit::Intent;
use super::text_input::{TextInput, TextInputEvent};
use gpui::{
    App, ClipboardItem, Context, Div, InteractiveElement as _, KeyBinding, Stateful, Window,
    actions, prelude::*,
};
use std::ops::Range;

actions!(
    text_input,
    [
        Backspace,
        Delete,
        DeleteWordLeft,
        DeleteWordRight,
        DeleteSpacesOrWordRight,
        DeleteToStart,
        DeleteToEnd,
        Left,
        Right,
        WordLeft,
        WordRight,
        Home,
        End,
        SelectLeft,
        SelectRight,
        SelectWordLeft,
        SelectWordRight,
        SelectHome,
        SelectEnd,
        SelectAll,
        Copy,
        Cut,
        Paste,
        Undo,
        Redo,
        Enter,
        SecondaryEnter,
        ShiftEnter,
        Escape,
        ShowCharacterPalette,
    ]
);

pub(crate) const KEY_CONTEXT: &str = "SkyeTextInput";

pub(super) fn bind_keys(cx: &mut App) {
    let keys = bindings(cfg!(target_os = "macos"));
    cx.bind_keys(keys);
}

/// The keys on macOS (`mac`) or elsewhere; a parameter so tests load both on any host.
pub(crate) fn bindings(mac: bool) -> Vec<KeyBinding> {
    macro_rules! keys {
        ($($key:literal => $action:expr),* $(,)?) => {
            vec![$(KeyBinding::new($key, $action, Some(KEY_CONTEXT))),*]
        };
    }
    let mut keys = keys![
        "backspace" => Backspace,
        "shift-backspace" => Backspace,
        "delete" => Delete,
        "shift-delete" => Delete,
        "enter" => Enter,
        "shift-enter" => ShiftEnter,
        "escape" => Escape,
        "left" => Left,
        "right" => Right,
        "shift-left" => SelectLeft,
        "shift-right" => SelectRight,
        "home" => Home,
        "end" => End,
        "shift-home" => SelectHome,
        "shift-end" => SelectEnd,
    ];
    keys.extend(if mac {
        keys![
            "ctrl-backspace" => Backspace,
            "cmd-backspace" => DeleteToStart,
            "cmd-delete" => DeleteToEnd,
            "alt-backspace" => DeleteWordLeft,
            "alt-delete" => DeleteWordRight,
            "cmd-enter" => SecondaryEnter,
            "alt-left" => WordLeft,
            "alt-right" => WordRight,
            "alt-shift-left" => SelectWordLeft,
            "alt-shift-right" => SelectWordRight,
            "cmd-left" => Home,
            "cmd-right" => End,
            "cmd-up" => Home,
            "cmd-down" => End,
            "ctrl-a" => Home,
            "ctrl-e" => End,
            "cmd-shift-left" => SelectHome,
            "cmd-shift-right" => SelectEnd,
            "cmd-shift-up" => SelectHome,
            "cmd-shift-down" => SelectEnd,
            "ctrl-shift-a" => SelectHome,
            "ctrl-shift-e" => SelectEnd,
            "cmd-a" => SelectAll,
            "cmd-c" => Copy,
            "cmd-x" => Cut,
            "cmd-v" => Paste,
            "cmd-z" => Undo,
            "cmd-shift-z" => Redo,
            "ctrl-cmd-space" => ShowCharacterPalette,
        ]
    } else {
        keys![
            "ctrl-backspace" => DeleteWordLeft,
            "ctrl-delete" => DeleteSpacesOrWordRight,
            "ctrl-enter" => SecondaryEnter,
            "ctrl-left" => WordLeft,
            "ctrl-right" => WordRight,
            "ctrl-shift-left" => SelectWordLeft,
            "ctrl-shift-right" => SelectWordRight,
            "ctrl-home" => Home,
            "ctrl-end" => End,
            "ctrl-shift-home" => SelectHome,
            "ctrl-shift-end" => SelectEnd,
            "ctrl-a" => SelectAll,
            "ctrl-c" => Copy,
            "ctrl-x" => Cut,
            "ctrl-v" => Paste,
            "ctrl-z" => Undo,
            "ctrl-y" => Redo,
            "ctrl-shift-z" => Redo,
        ]
    });
    keys
}

impl TextInput {
    fn delete(
        &mut self,
        intent: Intent,
        reach: impl FnOnce(&Self, usize) -> Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self.buffer.range();
        if selected.is_empty() {
            let range = reach(self, self.buffer.head());
            self.edit(range, "", intent, window, cx);
        } else {
            self.edit(selected, "", Intent::Atomic, window, cx);
        }
    }

    fn go(&mut self, to: usize, select: bool, window: &mut Window, cx: &mut Context<Self>) {
        if select {
            self.buffer.select_to(to);
        } else {
            self.buffer.move_to(to);
        }
        self.moved(window, cx);
    }

    fn selected_text(&self) -> Option<&str> {
        Some(&self.buffer.text()[self.buffer.range()]).filter(|t| !t.is_empty())
    }

    pub(super) fn has_selection(&self) -> bool {
        self.selected_text().is_some()
    }

    fn enter(&mut self, secondary: bool, shift: bool, cx: &mut Context<Self>) {
        self.buffer.end_step();
        // A dialog's confirm, a picker's choice: whoever binds Enter outside the field still gets it.
        cx.propagate();
        cx.emit(TextInputEvent::PressEnter { secondary, shift });
    }
}

pub(super) fn on_actions(
    el: Stateful<Div>,
    editable: bool,
    cx: &mut Context<TextInput>,
) -> Stateful<Div> {
    el.when(editable, |el| {
        el.on_action(cx.listener(|this, _: &Backspace, window, cx| {
            if this.buffer.range() == (0..0) {
                return cx.propagate();
            }
            let reach = |t: &TextInput, at| t.buffer.prev_grapheme(at)..at;
            this.delete(Intent::Backspace, reach, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Delete, window, cx| {
            let reach = |t: &TextInput, at| at..t.buffer.next_grapheme(at);
            this.delete(Intent::DeleteForward, reach, window, cx);
        }))
        .on_action(cx.listener(|this, _: &DeleteWordLeft, window, cx| {
            let reach = |t: &TextInput, at| t.buffer.prev_word(at)..at;
            this.delete(Intent::Atomic, reach, window, cx);
        }))
        .on_action(cx.listener(|this, _: &DeleteWordRight, window, cx| {
            let reach = |t: &TextInput, at| at..t.buffer.next_word(at);
            this.delete(Intent::Atomic, reach, window, cx);
        }))
        .on_action(
            cx.listener(|this, _: &DeleteSpacesOrWordRight, window, cx| {
                let reach = |t: &TextInput, at| match t.buffer.spaces_end(at) {
                    spaces if spaces > at => at..spaces,
                    _ => at..t.buffer.next_word(at),
                };
                this.delete(Intent::Atomic, reach, window, cx);
            }),
        )
        .on_action(cx.listener(|this, _: &DeleteToStart, window, cx| {
            this.delete(Intent::Atomic, |_, at| 0..at, window, cx);
        }))
        .on_action(cx.listener(|this, _: &DeleteToEnd, window, cx| {
            let reach = |t: &TextInput, at| at..t.buffer.text().len();
            this.delete(Intent::Atomic, reach, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Cut, window, cx| {
            if let Some(text) = this.selected_text() {
                cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
                this.edit(this.buffer.range(), "", Intent::Atomic, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &Paste, window, cx| {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text())
                && !text.is_empty()
            {
                this.edit(this.buffer.range(), &text, Intent::Atomic, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &Undo, window, cx| {
            let changed = this.buffer.undo();
            this.edited(changed, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Redo, window, cx| {
            let changed = this.buffer.redo();
            this.edited(changed, window, cx);
        }))
        .on_action(cx.listener(|this, _: &Enter, _, cx| this.enter(false, false, cx)))
        .on_action(cx.listener(|this, _: &SecondaryEnter, _, cx| this.enter(true, false, cx)))
        .on_action(cx.listener(|this, _: &ShiftEnter, _, cx| this.enter(false, true, cx)))
        .on_action(cx.listener(|this, _: &Escape, window, cx| {
            if this.buffer.marked().is_some() {
                this.end_composition(window, cx);
            }
            cx.propagate();
        }))
        .on_action(|_: &ShowCharacterPalette, window, _| window.show_character_palette())
    })
    .on_action(cx.listener(|this, _: &Left, window, cx| {
        let range = this.buffer.range();
        match range {
            Range { start: 0, end: 0 } => cx.propagate(),
            r if r.is_empty() => this.go(this.buffer.prev_grapheme(r.start), false, window, cx),
            r => this.go(r.start, false, window, cx),
        }
    }))
    .on_action(cx.listener(|this, _: &Right, window, cx| {
        let range = this.buffer.range();
        if range.start == this.buffer.text().len() {
            cx.propagate();
        } else if range.is_empty() {
            this.go(this.buffer.next_grapheme(range.end), false, window, cx);
        } else {
            this.go(range.end, false, window, cx);
        }
    }))
    .on_action(cx.listener(|this, _: &WordLeft, window, cx| {
        this.go(this.buffer.prev_word(this.buffer.head()), false, window, cx);
    }))
    .on_action(cx.listener(|this, _: &WordRight, window, cx| {
        this.go(this.buffer.next_word(this.buffer.head()), false, window, cx);
    }))
    .on_action(cx.listener(|this, _: &Home, window, cx| this.go(0, false, window, cx)))
    .on_action(cx.listener(|this, _: &End, window, cx| {
        this.go(this.buffer.text().len(), false, window, cx);
    }))
    .on_action(cx.listener(|this, _: &SelectLeft, window, cx| {
        this.go(
            this.buffer.prev_grapheme(this.buffer.head()),
            true,
            window,
            cx,
        );
    }))
    .on_action(cx.listener(|this, _: &SelectRight, window, cx| {
        this.go(
            this.buffer.next_grapheme(this.buffer.head()),
            true,
            window,
            cx,
        );
    }))
    .on_action(cx.listener(|this, _: &SelectWordLeft, window, cx| {
        this.go(this.buffer.prev_word(this.buffer.head()), true, window, cx);
    }))
    .on_action(cx.listener(|this, _: &SelectWordRight, window, cx| {
        this.go(this.buffer.next_word(this.buffer.head()), true, window, cx);
    }))
    .on_action(cx.listener(|this, _: &SelectHome, window, cx| this.go(0, true, window, cx)))
    .on_action(cx.listener(|this, _: &SelectEnd, window, cx| {
        this.go(this.buffer.text().len(), true, window, cx);
    }))
    .on_action(cx.listener(|this, _: &SelectAll, window, cx| this.select_all(window, cx)))
    .on_action(
        cx.listener(|this, _: &Copy, _, cx| match this.selected_text() {
            Some(text) => cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned())),
            // The window's own text selection (the timeline) copies instead.
            None => cx.propagate(),
        }),
    )
}
