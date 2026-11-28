use super::text_input_harness::{focused, input};
use gpui::{EntityInputHandler as _, TestAppContext};

#[gpui::test]
fn the_candidate_window_has_a_place_before_the_next_frame(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "abc");
    let field = input(&view, vcx);
    // One update: what the platform does inside one key event, with no frame in between.
    let rect = field.update_in(vcx, |i, window, cx| {
        let text = i.text_bounds().expect("drawn");
        i.replace_and_mark_text_in_range(None, "n", None, window, cx);
        i.bounds_for_range(3..3, text, window, cx)
    });
    let rect = rect.expect("no rect for the candidate window");
    assert!(rect.size.height > gpui::px(0.));
}
