//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 10: a text field is a named
//! `TextInput` node with its placeholder and value, takes `Focus` and `SetValue`, and gives its text runs with the
//! selection in characters. GPUI keeps a11y action delivery crate-private, so `SetValue` goes through
//! `TextInput::accessibility_set_value`, the one function our handler calls (the live check covers the real path).

use super::text_input_a11y;
use crate::ui::{self, TextField, TextInput, TextInputEvent};
use gpui::{
    AppContext as _, Context, Element as _, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Subscription, TestAppContext, VisualTestContext, Window, accesskit, div, px,
};
use gpui_base::test_support::snapshots;
use std::{cell::RefCell, rc::Rc};

struct Harness {
    field: Entity<TextInput>,
    disabled: Entity<TextInput>,
    changes: Rc<RefCell<usize>>,
    _sub: Subscription,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(px(14.));
        div()
            .w(px(300.))
            .child(TextField::new(&self.field).accessibility_label("Branch name"))
            .child(
                TextField::new(&self.disabled)
                    .disabled(true)
                    .accessibility_label("Locked"),
            )
    }
}

fn window(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    ui::button_test::setup(cx);
    let (view, vcx) = cx.add_window_view(|window, cx| {
        let field = cx.new(|cx| TextInput::new(window, cx).placeholder("main"));
        field.update(cx, |i, cx| i.set_value("dev", window, cx));
        let disabled = cx.new(|cx| TextInput::new(window, cx));
        let changes: Rc<RefCell<usize>> = Rc::default();
        let count = changes.clone();
        let sub = cx.subscribe_in(&field, window, move |_, _, e: &TextInputEvent, _, _| {
            if *e == TextInputEvent::Change {
                *count.borrow_mut() += 1;
            }
        });
        Harness {
            field,
            disabled,
            changes,
            _sub: sub,
        }
    });
    vcx.run_until_parked();
    (view, vcx)
}

/// The accesskit node the input's own element (before test support wraps it) writes (its role, name, value and actions).
fn node(input: &Entity<TextInput>, vcx: &mut VisualTestContext) -> accesskit::Node {
    input.update_in(vcx, |i, window, cx| {
        let el = i.element(window, cx);
        let mut node = accesskit::Node::new(el.a11y_role().expect("a role"));
        el.write_a11y_info(&mut node);
        node
    })
}

#[gpui::test]
fn a_field_is_a_named_text_input_with_its_value(cx: &mut TestAppContext) {
    let (view, vcx) = window(cx);
    let snaps = vcx.update(|window, _| snapshots(window));
    let field = snaps
        .iter()
        .find(|s| s.label() == Some("Branch name"))
        .expect("a node named Branch name");
    assert_eq!(field.role(), Some(gpui::Role::TextInput));
    assert_eq!(field.placeholder(), Some("main"));
    assert_eq!(field.value(), Some("dev"));

    let (input, locked) = view.read_with(vcx, |h, _| (h.field.clone(), h.disabled.clone()));
    let n = node(&input, vcx);
    assert!(n.supports_action(accesskit::Action::Focus));
    assert!(n.supports_action(accesskit::Action::SetValue));
    assert!(
        !node(&locked, vcx).supports_action(accesskit::Action::SetValue),
        "a disabled field can't be set"
    );

    input.update_in(vcx, |i, window, cx| {
        i.accessibility_set_value("feat", window, cx)
    });
    vcx.run_until_parked();
    assert_eq!(input.read_with(vcx, |i, _| i.value().to_string()), "feat");
    assert_eq!(
        *view.read_with(vcx, |h, _| h.changes.clone()).borrow(),
        1,
        "SetValue is an edit"
    );
}

#[test]
fn text_runs_count_graphemes_and_keep_the_selection_direction() {
    // a, 👍🏽 (8 bytes), e + combining accent (3 bytes): three characters.
    let text = "a👍🏽e\u{301}";
    let runs = text_input_a11y::runs(text, text.len(), 1);
    assert_eq!(runs.characters(), 3);
    assert_eq!(
        runs.selection(),
        (3, 1),
        "anchor at the end, focus after a: selected leftward"
    );
    let runs = text_input_a11y::runs(text, 1, 9);
    assert_eq!(runs.selection(), (1, 2));
}
