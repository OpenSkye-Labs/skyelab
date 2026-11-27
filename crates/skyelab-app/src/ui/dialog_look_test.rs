//! Own UI kit, stage 1 (.agent/plans/2026-11-27-own-ui-kit.md), criterion 7: a dialog keeps today's measures: the width
//! it asks for, its title 24 px inside its edge, footer buttons 8 px apart. (Its fade is `ui/motion_test.rs`.)

use super::dialog_test::{open, tree, window};
use gpui::{TestAppContext, accesskit, px};

#[gpui::test]
fn a_dialog_keeps_its_width_padding_and_footer_gap(cx: &mut TestAppContext) {
    let (_, vcx) = window(cx);
    open(vcx, "Rename thread");
    let nodes = tree(vcx);
    let find = |role: accesskit::Role, label: &str| {
        nodes
            .iter()
            .find(|s| s.role() == Some(role) && s.label() == Some(label))
            .unwrap_or_else(|| panic!("no {role:?} named {label:?}"))
            .bounds()
    };

    let dialog = find(accesskit::Role::Dialog, "Rename thread");
    assert_eq!(
        dialog.size.width,
        px(420.),
        "the width the dialog asked for: {dialog:?}"
    );
    let title = find(accesskit::Role::Heading, "Rename thread");
    assert_eq!(
        title.left() - dialog.left(),
        px(24.),
        "title inset: {title:?} in {dialog:?}"
    );
    let (cancel, rename) = (
        find(accesskit::Role::Button, "Cancel"),
        find(accesskit::Role::Button, "Rename"),
    );
    assert_eq!(rename.left() - cancel.right(), px(8.), "footer gap");
}
