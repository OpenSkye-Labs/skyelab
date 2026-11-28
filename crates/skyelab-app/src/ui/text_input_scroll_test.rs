//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 8: a line longer than its
//! field scrolls so the caret stays in view, and back.

use super::text_input_harness::{focused, input, range_bounds};
use gpui::{TestAppContext, px};

#[gpui::test]
fn the_caret_stays_in_view(cx: &mut TestAppContext) {
    let long = "abcdefghij".repeat(30);
    let (view, vcx) = focused(cx, 200., &long);
    let field = input(&view, vcx);
    vcx.simulate_keystrokes("end");
    let (text, caret) = field.read_with(vcx, |i, _| (i.text_bounds(), i.caret_bounds()));
    let (text, caret) = (text.expect("drawn"), caret.expect("a caret"));
    assert!(
        caret.left() >= text.left() && caret.right() <= text.right() + px(0.5),
        "caret {caret:?} inside {text:?} after End"
    );

    vcx.simulate_keystrokes("home");
    let first = range_bounds(&view, vcx, 0..1);
    let text = field.read_with(vcx, |i, _| i.text_bounds()).expect("drawn");
    assert_eq!(first.left(), text.left(), "Home scrolls back to the start");
}

#[gpui::test]
fn select_all_leaves_the_view_where_it_is(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., &"abcdefghij".repeat(30));
    vcx.simulate_keystrokes("home");
    vcx.simulate_keystrokes(&super::text_input_harness::cmd("a"));
    let text = input(&view, vcx).read_with(vcx, |i, _| i.text_bounds());
    let first = range_bounds(&view, vcx, 0..1);
    assert_eq!(first.left(), text.expect("drawn").left());
}

#[gpui::test]
fn a_field_that_narrows_keeps_its_caret_in_view(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 400., &"abcdefghij".repeat(30));
    vcx.simulate_keystrokes("end");
    view.update(vcx, |v, cx| {
        v.width = px(150.);
        cx.notify();
    });
    vcx.run_until_parked();
    let field = input(&view, vcx);
    let (text, caret) = field.read_with(vcx, |i, _| (i.text_bounds(), i.caret_bounds()));
    assert!(caret.expect("a caret").right() <= text.expect("drawn").right());
}
