use super::text_input_harness::{focused, input, selection};
use gpui::{TestAppContext, px};

#[gpui::test]
fn shift_end_shows_the_moving_end(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., &"abcdefghij".repeat(30));
    vcx.simulate_keystrokes("home shift-end");
    assert_eq!(selection(&view, vcx), 0..300);
    let field = input(&view, vcx);
    let (text, caret) = field.read_with(vcx, |i, _| (i.text_bounds(), i.caret_bounds()));
    let (text, caret) = (text.expect("drawn"), caret.expect("a caret"));
    assert!(
        caret.right() <= text.right() + px(0.5),
        "the caret is out of view after shift-end"
    );
}
