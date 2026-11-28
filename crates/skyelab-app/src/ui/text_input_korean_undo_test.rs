use super::text_input_harness::{cmd, focused, input, value};
use gpui::{EntityInputHandler as _, TestAppContext};

#[gpui::test]
fn a_rewritten_syllable_is_one_undo(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "");
    let field = input(&view, vcx);
    for (range, text) in [(None, "ㅎ"), (Some(0..1), "하"), (Some(0..1), "한")] {
        field.update_in(vcx, |i, window, cx| {
            i.replace_text_in_range(range.clone(), text, window, cx)
        });
    }
    assert_eq!(value(&view, vcx), "한");
    vcx.simulate_keystrokes(&cmd("z"));
    assert_eq!(value(&view, vcx), "", "one undo takes the typed syllable");
}
