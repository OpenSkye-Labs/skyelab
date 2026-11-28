//! SkyeLab's single-line text editing: the [`TextInput`] entity holds the text, the selection and the undo history,
//! takes keys (`text_input_keys`), the mouse (`text_input_mouse`) and the platform's input method
//! (`text_input_handler`), and draws itself (`text_input_element`). [`super::TextField`] puts the chrome around it.

use super::text_edit::{Buffer, Intent, fit};
use super::text_input_blink::Blink;
use super::text_input_element::{Layout, TextElement};
use super::{PopupMenu, text_input_a11y, text_input_keys};
use gpui::{
    AccessibleAction, App, Bounds, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point, Render,
    Role, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window, accesskit, div, prelude::*, px,
};
use gpui_base::TestSupportExt as _;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputEvent {
    /// The user (or an accessibility client) edited the text; never sent for [`TextInput::set_value`].
    Change,
    /// `secondary` is ⌘ on macOS, Ctrl elsewhere.
    PressEnter {
        secondary: bool,
        shift: bool,
    },
    Focus,
    Blur,
}

/// What the element around the input ([`super::TextField`]) says about it, written each frame.
#[derive(Default)]
pub(super) struct Frame {
    pub label: Option<SharedString>,
    pub accessibility_id: Option<SharedString>,
    pub disabled: bool,
}

pub(super) struct Menu {
    pub at: Point<Pixels>,
    pub menu: Entity<PopupMenu>,
    pub _dismiss: Subscription,
}

/// How far a disabled field's text and its prefix and suffix fade.
pub(super) const DISABLED_TEXT: f32 = 0.5;

pub struct TextInput {
    pub(super) buffer: Buffer,
    pub(super) focus: FocusHandle,
    pub(super) placeholder: SharedString,
    pub(super) frame: Frame,
    pub(super) blink: Blink,
    pub(super) layout: Option<Layout>,
    pub(super) scroll_x: Pixels,
    /// The (anchor, head) the view last followed, so a wheel scroll isn't pulled back; `None` to follow again.
    pub(super) revealed: Option<(usize, usize)>,
    /// The caret place the platform last heard, for the input method's window.
    pub(super) ime_anchor: Option<Bounds<Pixels>>,
    pub(super) selecting: bool,
    /// The word a double click took: dragging on selects by words from it.
    pub(super) word: Option<Range<usize>>,
    pub(super) menu: Option<Menu>,
    /// The focus went to our own menu, which is not a blur.
    pub(super) blur_held: bool,
    _focus: [Subscription; 2],
}

impl EventEmitter<TextInputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TextInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let on_focus = cx.on_focus(&focus, window, |this, _, cx| {
            this.start_blink(cx);
            if !std::mem::take(&mut this.blur_held) {
                cx.emit(TextInputEvent::Focus);
            }
        });
        let on_blur = cx.on_blur(&focus, window, |this, _, cx| {
            this.selecting = false;
            this.buffer.unmark();
            this.stop_blink(cx);
            if this.menu.is_some() {
                this.blur_held = true;
            } else {
                cx.emit(TextInputEvent::Blur);
            }
            cx.notify();
        });
        Self {
            buffer: Buffer::default(),
            focus,
            placeholder: SharedString::default(),
            frame: Frame::default(),
            blink: Blink::default(),
            layout: None,
            scroll_x: px(0.),
            revealed: None,
            ime_anchor: None,
            selecting: false,
            word: None,
            menu: None,
            blur_held: false,
            _focus: [on_focus, on_blur],
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// The text it starts with, the caret before it.
    pub fn default_value(mut self, value: impl AsRef<str>) -> Self {
        self.buffer.set_text(value.as_ref());
        self.buffer.move_to(0);
        self
    }

    pub fn value(&self) -> SharedString {
        self.buffer.text().to_owned().into()
    }

    /// Replaces the text: no [`TextInputEvent::Change`], the undo history starts over, the caret goes to the end and
    /// the view to the start.
    pub fn set_value(
        &mut self,
        value: impl AsRef<str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.buffer.set_text(value.as_ref());
        self.scroll_x = px(0.);
        self.revealed = Some((self.buffer.anchor(), self.buffer.head()));
        window.notify_text_input_changed(&self.focus);
        cx.notify();
    }

    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub(super) fn is_disabled(&self) -> bool {
        self.frame.disabled
    }

    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
    }

    pub fn select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select(0, self.buffer.text().len());
        self.moved(window, cx);
        // The view stays where it is, as in every text field.
        self.revealed = Some((self.buffer.anchor(), self.buffer.head()));
    }

    /// In bytes.
    #[cfg(test)]
    pub fn selected_range(&self) -> Range<usize> {
        self.buffer.range()
    }

    /// The moving end of the selection, in bytes.
    #[cfg(test)]
    pub fn cursor(&self) -> usize {
        self.buffer.head()
    }

    /// Where the text shows, in the last frame.
    #[cfg(test)]
    pub fn text_bounds(&self) -> Option<Bounds<Pixels>> {
        self.layout.as_ref().map(|l| l.bounds)
    }

    /// Where `range` was drawn in the last frame.
    #[cfg(test)]
    pub fn range_bounds(&self, range: Range<usize>) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        (layout.text == self.buffer.text()).then(|| layout.range_bounds(&range))
    }

    /// The caret's place in the last frame, if it had one.
    #[cfg(test)]
    pub fn caret_bounds(&self) -> Option<Bounds<Pixels>> {
        self.layout.as_ref()?.caret
    }

    /// Whether the blink is in its shown phase.
    #[cfg(test)]
    pub fn caret_visible(&self) -> bool {
        self.blink.visible
    }

    /// An accessibility client's `SetValue`: an edit, so it can be undone and emits `Change`.
    pub fn accessibility_set_value(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_disabled() {
            self.edit(
                0..self.buffer.text().len(),
                value,
                Intent::Atomic,
                window,
                cx,
            );
        }
    }

    /// The text and the caret may have changed: the caret shows, the view follows it and the platform hears of it.
    pub(super) fn moved(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pause_blink(cx);
        self.revealed = None;
        window.notify_text_selection_changed(&self.focus);
        cx.notify();
    }

    pub(super) fn edited(&mut self, changed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if changed {
            window.notify_text_input_changed(&self.focus);
            cx.emit(TextInputEvent::Change);
        }
        self.moved(window, cx);
    }

    pub(super) fn edit(
        &mut self,
        range: Range<usize>,
        new: &str,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let changed = self.buffer.replace(range, &fit(new), intent);
        self.edited(changed, window, cx);
    }

    /// The element, before test support wraps it.
    pub(super) fn element(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let disabled = self.is_disabled();
        let exposed = window.is_a11y_active() || cfg!(test);
        let runs = window
            .is_a11y_active()
            .then(|| text_input_a11y::runs_of(self, window.scale_factor()));
        let placeholder = Some(self.placeholder.clone()).filter(|p| !p.is_empty());
        let label = self.frame.label.clone().or_else(|| placeholder.clone());
        let el = div()
            .id("text-input")
            // Keys meant for our open menu must not edit the text behind it.
            .when(self.menu.is_none(), |d| {
                d.key_context(text_input_keys::KEY_CONTEXT)
            })
            .role(Role::TextInput)
            .when_some(label, |d, label| d.aria_label(label))
            .when_some(placeholder, |d, p| d.aria_placeholder(p))
            .when_some(self.frame.accessibility_id.clone(), |d, id| {
                d.accessibility_id(id)
            })
            .when(exposed, |d| d.aria_value(self.value()))
            .when_some(runs, |d, runs| {
                d.a11y_synthetic_children(move |builder| runs.push(builder))
            })
            .when(!disabled, |d| {
                let (focus, set) = (cx.entity(), cx.entity());
                d.on_a11y_action(AccessibleAction::Focus, move |_, window, cx| {
                    focus.update(cx, |this, cx| this.focus(window, cx));
                })
                .on_a11y_action(AccessibleAction::SetValue, move |data, window, cx| {
                    if let Some(accesskit::ActionData::Value(value)) = data {
                        set.update(cx, |this, cx| {
                            this.accessibility_set_value(value, window, cx);
                        });
                    }
                })
                .cursor_text()
            })
            .flex()
            .flex_1()
            .min_h_full()
            .items_center()
            .overflow_x_hidden()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_scroll(cx.listener(Self::wheel));
        text_input_keys::on_actions(el, !disabled, cx)
            .child(TextElement::new(cx.entity()))
            .children(self.render_menu())
    }
}

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus.clone().tab_stop(!self.is_disabled());
        self.element(window, cx).test_support().track_focus(&focus)
    }
}
