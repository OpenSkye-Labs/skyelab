//! Review proof: a dialog that is fading out must not take clicks.
use super::dialog_test::{tree, window};
use crate::ui::{self, Button};
use gpui::{Modifiers, ParentElement as _, TestAppContext, accesskit, px};
use std::cell::Cell;
use std::rc::Rc;

#[gpui::test]
fn a_closing_dialogs_buttons_take_no_clicks(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    let allowed = Rc::new(Cell::new(0usize));
    let count = allowed.clone();
    vcx.update(|window, cx| {
        ui::open_dialog(window, cx, move |dialog, _, _| {
            let count = count.clone();
            dialog.title("Allow example.test?").w(px(420.)).footer(
                ui::dialog_footer()
                    .child(ui::dialog_cancel("cancel"))
                    .child(
                        Button::new("allow")
                            .label("Allow")
                            .on_click(move |_, window, cx| {
                                count.set(count.get() + 1);
                                ui::close_dialog(window, cx);
                            }),
                    ),
            )
        })
    });
    vcx.run_until_parked();
    let at = tree(vcx)
        .iter()
        .find(|s| s.role() == Some(accesskit::Role::Button) && s.label() == Some("Allow"))
        .expect("the Allow button is drawn")
        .bounds()
        .center();

    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(allowed.get(), 1, "first click allows");
    // No clock advance: the dialog is still fading out.
    vcx.simulate_click(at, Modifiers::none());
    vcx.run_until_parked();
    assert_eq!(allowed.get(), 1, "FADING_DIALOG_TOOK_A_CLICK");
}
