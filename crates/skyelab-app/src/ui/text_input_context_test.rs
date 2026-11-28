//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 15: the workspace's field
//! bindings reach our input's `SkyeTextInput` context, so off macOS `alt-left` in a field doesn't go back a page
//! (it did nothing there with gpui-component's `Input` context), and on macOS `cmd-[` still does. Each keymap is loaded
//! on its own (the platform is a parameter, lesson 46), so both run on any host.

use crate::ui::{self, TextField, TextInput};
use crate::workspace::GoBack;
use gpui::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window,
    div, px,
};
use std::{cell::Cell, rc::Rc};

struct Page {
    input: Entity<TextInput>,
    backs: Rc<Cell<usize>>,
}

impl Render for Page {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(14.));
        let backs = self.backs.clone();
        div()
            .key_context("Workspace")
            .on_action(move |_: &GoBack, _, _| backs.set(backs.get() + 1))
            .w(px(300.))
            .child(TextField::new(&self.input).accessibility_label("Search"))
    }
}

fn page(cx: &mut TestAppContext, mac: bool) -> (Entity<Page>, &mut VisualTestContext) {
    ui::button_test::setup(cx);
    let back = if mac { "cmd-[" } else { "alt-left" };
    cx.update(|cx| {
        cx.clear_key_bindings();
        let ours = ui::text_input_keys::bindings(mac);
        cx.bind_keys(ours);
        cx.bind_keys([KeyBinding::new(back, GoBack, Some("Workspace"))]);
        cx.bind_keys(crate::workspace::field_bindings(mac));
    });
    let (view, vcx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| TextInput::new(window, cx));
        input.update(cx, |i, cx| i.set_value("ab cd", window, cx));
        Page {
            input,
            backs: Rc::default(),
        }
    });
    let input = view.read_with(vcx, |p, _| p.input.clone());
    vcx.update(|window, cx| {
        let f = input.focus_handle(cx);
        window.focus(&f, cx);
    });
    vcx.run_until_parked();
    (view, vcx)
}

fn cursor(view: &Entity<Page>, vcx: &mut VisualTestContext) -> usize {
    let input = view.read_with(vcx, |p, _| p.input.clone());
    input.read_with(vcx, |i, _| i.cursor())
}

#[gpui::test]
fn off_macos_alt_left_in_a_field_stays_on_the_page(cx: &mut TestAppContext) {
    let (view, vcx) = page(cx, false);
    let backs = view.read_with(vcx, |p, _| p.backs.clone());
    vcx.simulate_keystrokes("alt-left");
    assert_eq!(
        backs.get(),
        0,
        "alt-left in a field must not go back a page"
    );
    assert_eq!(cursor(&view, vcx), 5, "and it isn't a caret move off macOS");
    vcx.simulate_keystrokes("ctrl-left");
    assert_eq!(cursor(&view, vcx), 3, "ctrl-left moves a word");
}

#[gpui::test]
fn on_macos_cmd_bracket_in_a_field_goes_back(cx: &mut TestAppContext) {
    let (view, vcx) = page(cx, true);
    let backs = view.read_with(vcx, |p, _| p.backs.clone());
    vcx.simulate_keystrokes("cmd-[");
    assert_eq!(backs.get(), 1, "history wins in a field on macOS");
    vcx.simulate_keystrokes("alt-left");
    assert_eq!(cursor(&view, vcx), 3, "alt-left moves a word on macOS");
}

#[gpui::test]
fn off_macos_a_forward_word_delete_stops_after_the_spaces(cx: &mut TestAppContext) {
    let (view, vcx) = page(cx, false);
    let input = view.read_with(vcx, |p, _| p.input.clone());
    vcx.simulate_keystrokes("home right right ctrl-delete");
    assert_eq!(input.read_with(vcx, |i, _| i.value().to_string()), "abcd");
}
