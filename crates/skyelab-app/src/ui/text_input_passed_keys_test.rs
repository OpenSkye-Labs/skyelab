//! Own UI kit, stage 6a: the keys a field leaves to what is around it (a picker's arrows, a dialog's Enter, the
//! palette's Backspace, the window's copy) are the ones gpui-component's field left.

use super::{TextField, TextInput};
use gpui::{
    AnyElement, AppContext as _, Context, Focusable as _, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::input::{Input, InputState};
use std::{cell::Cell, rc::Rc};

gpui::actions!(text_input_passed_keys_test, [Passed]);

#[derive(Clone, Copy, Debug)]
enum At {
    Start,
    End,
    All,
}

struct Page {
    field: Box<dyn Fn() -> AnyElement>,
    passed: Rc<Cell<usize>>,
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let passed = self.passed.clone();
        div()
            .key_context("KeyParent")
            .on_action(move |_: &Passed, _, _| passed.set(passed.get() + 1))
            .w(px(300.))
            .child((self.field)())
    }
}

fn keys() -> Vec<String> {
    let cmd = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    let plain = "escape enter shift-enter up down shift-up shift-down pageup pagedown left right home end \
                 shift-left shift-right backspace delete f5";
    let with_cmd = "a c x v z f enter up down left right backspace shift-f [ ] .";
    let plain = plain.split_whitespace().map(str::to_owned);
    plain
        .chain(with_cmd.split_whitespace().map(|k| format!("{cmd}-{k}")))
        .collect()
}

/// Which of `keys()` reach the parent's binding, pressed in a field holding `text` with the caret `at`.
fn passed(cx: &mut TestAppContext, ours: bool, text: &'static str, at: At) -> Vec<&'static str> {
    let mut out = Vec::new();
    for key in keys() {
        let key: &'static str = Box::leak(key.into_boxed_str());
        let passed: Rc<Cell<usize>> = Rc::default();
        let count = passed.clone();
        let binding = KeyBinding::new(key, Passed, Some("KeyParent"));
        cx.update(|cx| cx.bind_keys([binding]));
        let (_, vcx) = cx.add_window_view(move |window, cx| {
            let (field, focus): (Box<dyn Fn() -> AnyElement>, _) = if ours {
                let state = cx.new(|cx| TextInput::new(window, cx).default_value(text));
                let focus = state.focus_handle(cx);
                (
                    Box::new(move || TextField::new(&state).into_any_element()),
                    focus,
                )
            } else {
                let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
                let focus = state.focus_handle(cx);
                (
                    Box::new(move || Input::new(&state).into_any_element()),
                    focus,
                )
            };
            window.focus(&focus, cx);
            Page {
                field,
                passed: count,
            }
        });
        vcx.run_until_parked();
        // Without a move, the caret is where `default_value` put it.
        match at {
            At::Start => {}
            At::End => vcx.simulate_keystrokes("end"),
            At::All => vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            }),
        }
        passed.set(0);
        vcx.simulate_keystrokes(key);
        if passed.get() > 0 {
            out.push(key);
        }
    }
    out
}

#[gpui::test]
fn the_keys_left_to_the_parent_are_theirs(cx: &mut TestAppContext) {
    super::button_test::setup(cx);
    for (text, at) in [
        ("", At::Start),
        ("abc", At::Start),
        ("abc", At::End),
        ("abc", At::All),
    ] {
        let theirs = passed(cx, false, text, at);
        let ours = passed(cx, true, text, at);
        assert!(theirs.contains(&"escape"), "the probe sees passed keys");
        assert_eq!(ours, theirs, "text {text:?}, caret {at:?}");
    }
}
