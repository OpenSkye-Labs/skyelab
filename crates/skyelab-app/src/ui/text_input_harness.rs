//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md): the window the text input's
//! acceptance tests type into. One `ui::TextField` over a `ui::TextInput`, drawn at the app's 14 px rem, with every
//! event the input emits recorded and the parent's renders counted.

use crate::ui::{self, TextField, TextInput, TextInputEvent};
use gpui::{
    AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, Styled as _, Subscription, TestAppContext,
    VisualTestContext, Window, div, px,
};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::Rc,
};

// What a parent of the field binds in its own `FieldParent` context (a dialog's confirm, a page's Escape).
gpui::actions!(text_input_harness, [ParentEnter, ParentEscape]);

pub(super) struct Field {
    pub input: Entity<TextInput>,
    pub events: Rc<RefCell<Vec<TextInputEvent>>>,
    pub disabled: bool,
    pub renders: Rc<RefCell<usize>>,
    /// `ParentEnter` / `ParentEscape` handled by the field's parent.
    pub enters: Rc<Cell<usize>>,
    pub escapes: Rc<Cell<usize>>,
    /// Another tab stop, so a test can move the focus out of the field.
    pub other: FocusHandle,
    /// A cached view beside the field; it renders again only if something redraws more than the field.
    pub sibling: Entity<Sibling>,
    width: Pixels,
    _sub: Subscription,
}

impl Render for Field {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        *self.renders.borrow_mut() += 1;
        window.set_rem_size(px(14.));
        let (enters, escapes) = (self.enters.clone(), self.escapes.clone());
        div()
            .key_context("FieldParent")
            .on_action(move |_: &ParentEnter, _, _| enters.set(enters.get() + 1))
            .on_action(move |_: &ParentEscape, _, _| escapes.set(escapes.get() + 1))
            .flex()
            .flex_col()
            .child(
                div().w(self.width).child(
                    TextField::new(&self.input)
                        .disabled(self.disabled)
                        .accessibility_label("Field"),
                ),
            )
            .child(div().id("other").track_focus(&self.other).size(px(10.)))
            .child(ui::cached_view(
                self.sibling.clone().into(),
                gpui::StyleRefinement::default(),
                window,
            ))
    }
}

pub(super) struct Sibling {
    pub renders: Rc<Cell<usize>>,
}

impl Render for Sibling {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div().size(px(10.))
    }
}

impl Field {
    fn new(text: &str, width: f32, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |i, cx| i.set_value(text, window, cx));
        let events: Rc<RefCell<Vec<TextInputEvent>>> = Rc::default();
        let log = events.clone();
        let sub = cx.subscribe_in(&input, window, move |_, _, e: &TextInputEvent, _, _| {
            log.borrow_mut().push(e.clone());
        });
        Self {
            input,
            events,
            disabled: false,
            renders: Rc::default(),
            enters: Rc::default(),
            escapes: Rc::default(),
            other: cx.focus_handle().tab_stop(true),
            sibling: cx.new(|_| Sibling {
                renders: Rc::default(),
            }),
            width: px(width),
            _sub: sub,
        }
    }
}

/// A `width` px field holding `text` (set with `set_value`, so no event), not focused yet.
pub(super) fn field<'a>(
    cx: &'a mut TestAppContext,
    width: f32,
    text: &str,
) -> (Entity<Field>, &'a mut VisualTestContext) {
    ui::button_test::setup(cx);
    let text = text.to_string();
    let made: Rc<RefCell<Option<Entity<Field>>>> = Rc::default();
    let out = made.clone();
    // In the app's window root, which binds Tab.
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        let view = cx.new(|cx| Field::new(&text, width, window, cx));
        *out.borrow_mut() = Some(view.clone());
        ui::window_root(view.into(), window, cx)
    });
    let view = made.take().expect("made with the window");
    // GPUI reports focus changes, and the caret shows, only in the active window.
    vcx.update(|window, _| window.activate_window());
    vcx.run_until_parked();
    (view, vcx)
}

/// The same field, focused.
pub(super) fn focused<'a>(
    cx: &'a mut TestAppContext,
    width: f32,
    text: &str,
) -> (Entity<Field>, &'a mut VisualTestContext) {
    let (view, vcx) = field(cx, width, text);
    let input = view.read_with(vcx, |v, _| v.input.clone());
    vcx.update(|window, cx| {
        let focus = input.focus_handle(cx);
        window.focus(&focus, cx);
    });
    vcx.run_until_parked();
    view.read_with(vcx, |v, _| v.events.borrow_mut().clear());
    (view, vcx)
}

pub(super) fn input(view: &Entity<Field>, vcx: &mut VisualTestContext) -> Entity<TextInput> {
    view.read_with(vcx, |v, _| v.input.clone())
}

pub(super) fn value(view: &Entity<Field>, vcx: &mut VisualTestContext) -> String {
    let input = input(view, vcx);
    input.read_with(vcx, |i, _| i.value().to_string())
}

pub(super) fn selection(view: &Entity<Field>, vcx: &mut VisualTestContext) -> Range<usize> {
    let input = input(view, vcx);
    input.read_with(vcx, |i, _| i.selected_range())
}

pub(super) fn cursor(view: &Entity<Field>, vcx: &mut VisualTestContext) -> usize {
    let input = input(view, vcx);
    input.read_with(vcx, |i, _| i.cursor())
}

pub(super) fn events(view: &Entity<Field>, vcx: &mut VisualTestContext) -> Vec<TextInputEvent> {
    view.read_with(vcx, |v, _| v.events.borrow_mut().drain(..).collect())
}

/// Where the bytes `range` were drawn in the last frame.
pub(super) fn range_bounds(
    view: &Entity<Field>,
    vcx: &mut VisualTestContext,
    range: Range<usize>,
) -> Bounds<Pixels> {
    let input = input(view, vcx);
    input
        .read_with(vcx, |i, _| i.range_bounds(range.clone()))
        .unwrap_or_else(|| panic!("{range:?} was drawn"))
}

/// `mac` on macOS, `other` elsewhere: the two keymaps differ (alt- vs ctrl- word moves, cmd- vs ctrl- edits).
pub(super) fn key(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// The platform's command key plus `k` (`cmd-` on macOS, `ctrl-` elsewhere).
pub(super) fn cmd(k: &str) -> String {
    format!("{}-{k}", key("cmd", "ctrl"))
}
