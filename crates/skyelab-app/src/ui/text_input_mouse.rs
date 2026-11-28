//! The text input under the pointer: click, drag, double and triple click, the wheel, and the right-click menu.

use super::menu::POPUP_PRIORITY;
use super::text_input::{Menu, TextInput, TextInputEvent};
use super::text_input_keys::{Copy, Cut, Paste, SelectAll};
use super::{PopupMenu, PopupMenuItem};
use gpui::{
    Context, DismissEvent, Focusable as _, IntoElement, MouseButton, MouseDownEvent, Pixels, Point,
    ScrollEvent, Window, anchored, deferred, div, prelude::*, px,
};

impl TextInput {
    fn offset_at(&self, at: Point<Pixels>) -> usize {
        let Some(layout) = self.layout.as_ref() else {
            return 0;
        };
        if layout.text != self.buffer.text() {
            return self.buffer.head();
        }
        self.buffer.snap(layout.offset_at(at.x))
    }

    pub(super) fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus.focus(window, cx);
        self.start_blink(cx);
        // The window's own text selection (the timeline's) must not start a drag from here.
        gpui_base::GlobalState::suppress_text_selection(cx);
        let at = self.offset_at(event.position);
        if event.button == MouseButton::Right {
            if !self.buffer.range().contains(&at) {
                self.buffer.move_to(at);
            }
            self.moved(window, cx);
            // GPUI focuses the pressed element after this, which would take the focus back from the menu.
            window.prevent_default();
            return self.open_menu(event.position, window, cx);
        }
        self.selecting = true;
        self.word = None;
        match event.click_count {
            3.. => self.buffer.select(0, self.buffer.text().len()),
            2 => {
                let word = self.buffer.word_at(at);
                self.buffer.select(word.start, word.end);
                self.word = Some(word);
            }
            _ if event.modifiers.shift => self.buffer.select_to(at),
            _ => self.buffer.move_to(at),
        }
        self.moved(window, cx);
    }

    /// The pointer moved with the button down, anywhere in the window.
    pub(super) fn drag_to(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.offset_at(at);
        match self.word.clone() {
            Some(word) if at < word.start => {
                self.buffer.select(word.end, self.buffer.word_at(at).start);
            }
            Some(word) => {
                let to = self.buffer.word_at(at).end.max(word.end);
                self.buffer
                    .select(word.start, if at <= word.end { word.end } else { to });
            }
            None => self.buffer.select_to(at),
        }
        self.moved(window, cx);
    }

    pub(super) fn wheel(
        &mut self,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(layout) = self.layout.as_ref() else {
            return;
        };
        let delta = event.delta.pixel_delta(window.line_height());
        let x = layout.clamp_scroll(self.scroll_x + delta.x);
        // A scroll that moves nothing here is the page's.
        if x != self.scroll_x {
            self.scroll_x = x;
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn open_menu(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.focus.clone();
        let selected = self.has_selection();
        let can_paste = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .is_some_and(|t| !t.is_empty());
        let menu = PopupMenu::build(window, cx, move |menu, _, _| {
            let item = |label: &'static str, action: Box<dyn gpui::Action>, on: bool| {
                PopupMenuItem::new(label).action(action).disabled(!on)
            };
            menu.action_context(focus)
                .item(item("Cut", Box::new(Cut), selected))
                .item(item("Copy", Box::new(Copy), selected))
                .item(item("Paste", Box::new(Paste), can_paste))
                .separator()
                .item(item("Select All", Box::new(SelectAll), true))
        });
        menu.focus_handle(cx).focus(window, cx);
        let _dismiss = cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
            this.menu = None;
            cx.notify();
            // The focus went on to something else: the blur held while the menu was open happened after all.
            cx.defer_in(window, |this, window, cx| {
                if !this.focus.is_focused(window) && std::mem::take(&mut this.blur_held) {
                    cx.emit(TextInputEvent::Blur);
                }
            });
        });
        self.menu = Some(Menu { at, menu, _dismiss });
        cx.notify();
    }

    pub(super) fn render_menu(&self) -> Option<impl IntoElement + use<>> {
        let Menu { at, menu, .. } = self.menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(*at)
                    .snap_to_window_with_margin(px(8.))
                    .child(div().occlude().child(menu.clone())),
            )
            .with_priority(POPUP_PRIORITY),
        )
    }
}
