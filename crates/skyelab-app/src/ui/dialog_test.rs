//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 2: a dialog is one `Role::Dialog` node named
//! by its title, and its ✕ is named after it, never plain "Close" (the window's own button). Also the test window the
//! other dialog tests share.

use crate::ui::{self, Button};
use gpui::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, TestAppContext,
    VisualTestContext, Window, accesskit, div, px,
};
use gpui_base::test_support::{ElementSnapshot, snapshots};

/// The window's content: one tab stop, "Behind", and a line of selectable text.
pub(super) struct Behind {
    pub(super) focus: FocusHandle,
}

impl Render for Behind {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(
                div()
                    .id("behind")
                    .role(accesskit::Role::Button)
                    .aria_label("Behind")
                    .track_focus(&self.focus)
                    .on_click(|_, _, _| {})
                    .child("Behind"),
            )
            .child(div().debug_selector(|| "behind-text".into()).child(
                gpui_base::SelectableText::new("behind-text", "Text behind the dialog"),
            ))
    }
}

/// A window built the way SkyeLab builds every window (`ui::window_root`), holding [`Behind`].
pub(super) fn window(cx: &mut TestAppContext) -> (Entity<Behind>, &mut VisualTestContext) {
    ui::button_test::setup(cx);
    let behind = cx.update(|cx| {
        cx.new(|cx| Behind {
            focus: cx.focus_handle().tab_stop(true),
        })
    });
    let content = behind.clone();
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.run_until_parked();
    (behind, vcx)
}

/// Opens a dialog titled `title`: a line of text, Cancel and Rename.
pub(super) fn open(vcx: &mut VisualTestContext, title: &'static str) {
    vcx.update(|window, cx| {
        ui::open_dialog(window, cx, move |dialog, _, _| {
            dialog
                .title(title)
                .w(px(420.))
                .child(div().child("Name it."))
                .footer(
                    ui::dialog_footer()
                        .child(ui::dialog_cancel("cancel"))
                        .child(Button::new("ok").label("Rename").on_click(|_, _, _| {})),
                )
        })
    });
    vcx.run_until_parked();
}

/// Lets a closing dialog's fade run out.
pub(super) fn settle(vcx: &mut VisualTestContext) {
    vcx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
}

pub(super) fn tree(vcx: &mut VisualTestContext) -> Vec<ElementSnapshot> {
    vcx.update(|window, _| snapshots(window))
}

/// The labels of the dialog nodes in the tree, sorted (snapshots come in no particular order).
pub(super) fn dialogs(vcx: &mut VisualTestContext) -> Vec<String> {
    let mut labels: Vec<String> = tree(vcx)
        .iter()
        .filter(|s| s.role() == Some(accesskit::Role::Dialog))
        .map(|s| s.label().unwrap_or_default().to_owned())
        .collect();
    labels.sort();
    labels
}

pub(super) fn behind_focused(behind: &Entity<Behind>, vcx: &mut VisualTestContext) -> bool {
    vcx.update(|window, cx| behind.read(cx).focus.is_focused(window))
}

#[gpui::test]
fn a_dialog_is_named_by_its_title_and_so_is_its_close_button(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    open(vcx, "Rename thread");

    assert_eq!(
        dialogs(vcx),
        ["Rename thread"],
        "one dialog node, named by its title"
    );
    let buttons: Vec<String> = tree(vcx)
        .iter()
        .filter(|s| s.role() == Some(accesskit::Role::Button))
        .filter_map(|s| s.label().map(str::to_owned))
        .collect();
    assert!(
        buttons.iter().any(|b| b == "Close Rename thread"),
        "the ✕ is named after the dialog: {buttons:?}"
    );
    assert!(
        !buttons.iter().any(|b| b == "Close"),
        "no button in a dialog is named plain \"Close\": {buttons:?}"
    );
}
