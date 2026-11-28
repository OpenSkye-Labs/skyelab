//! Review proof: a drag end takes the character whose middle it crossed.

use super::changes_model::LineKind;
use super::diff_viewer::DiffViewer;
use crate::ui;
use gpui::{AppContext as _, ClipboardItem, Modifiers, MouseButton, TestAppContext, point};

#[gpui::test]
fn a_drag_takes_the_character_it_crossed_the_middle_of(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        ui::install_theme(cx).expect("the bundled themes parse");
        ui::bind_keys(cx);
        super::diff_viewer::bind_keys(cx);
    });
    let lines = vec![(None, Some(1), LineKind::Added, "abcdefghij".to_owned())];
    let v = cx.update(|cx| cx.new(|cx| DiffViewer::new("/r/a.txt", lines, Vec::new(), cx)));
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(v.clone().into(), window, cx));
    vcx.run_until_parked();
    let text = vcx
        .debug_bounds("diff-viewer-text-0")
        .expect("control: the text");
    let at = |col: f32| point(text.left() + text.size.width * (col / 10.), text.center().y);
    vcx.simulate_mouse_down(at(0.1), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_move(at(6.9), MouseButton::Left, Modifiers::none());
    vcx.simulate_mouse_up(at(6.9), MouseButton::Left, Modifiers::none());
    vcx.run_until_parked();
    vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("nothing copied".into())));
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    let copied = vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()));
    assert!(
        copied.as_deref() != Some("nothing copied"),
        "control: something was copied"
    );
    assert_eq!(
        copied.as_deref(),
        Some("abcdefg"),
        "the drag crossed most of the g"
    );
}
