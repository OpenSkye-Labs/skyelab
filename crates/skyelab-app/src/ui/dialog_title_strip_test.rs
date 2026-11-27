//! Review proof: a press in the window's title strip leaves the dialog open (it drags the window).
use super::dialog_test::{dialogs, open, settle, window};
use gpui::{Modifiers, TestAppContext, point, px};

#[gpui::test]
fn a_press_in_the_title_strip_leaves_the_dialog_open(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    open(vcx, "Rename thread");
    vcx.simulate_click(point(px(5.), px(10.)), Modifiers::none());
    settle(vcx);
    assert_eq!(
        dialogs(vcx),
        ["Rename thread"],
        "a press in the title strip closed the dialog"
    );
}
