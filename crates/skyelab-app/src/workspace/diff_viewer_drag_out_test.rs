//! Review proof: a drag that starts in the viewer and ends outside it keeps the viewer's selection.

use super::changes_model::LineKind;
use super::diff_viewer::DiffViewer;
use crate::ui;
use gpui::{
    AppContext as _, ClipboardItem, Context, IntoElement, Modifiers, MouseButton, Render,
    TestAppContext, VisualTestContext, Window, div, point, prelude::*, px, size,
};

struct Behind;
impl Render for Behind {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            div().absolute().top(px(200.)).left(px(20.)).child(
                gpui_base::SelectableText::new("behind-text", "Text behind the dialog"),
            ),
        )
    }
}

fn copied(vcx: &mut VisualTestContext) -> String {
    vcx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("nothing copied".into())));
    vcx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    vcx.update(|_, cx| cx.read_from_clipboard().and_then(|c| c.text()))
        .unwrap_or_default()
}

#[gpui::test]
fn a_drag_that_leaves_the_viewer_keeps_its_selection(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        ui::install_theme(cx).expect("the bundled themes parse");
        ui::bind_keys(cx);
        super::diff_viewer::bind_keys(cx);
    });
    let lines = (1..=100u32)
        .map(|i| (None, Some(i), LineKind::Added, format!("fn line_{i}() {{}}")))
        .collect();
    let v = cx.update(|cx| cx.new(|cx| DiffViewer::new("/r/a.rs", lines, Vec::new(), cx)));
    let behind = cx.update(|cx| cx.new(|_| Behind));
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(behind.clone().into(), window, cx));
    vcx.simulate_resize(size(px(1200.), px(800.)));
    let shown = v.clone();
    vcx.update(|window, cx| {
        ui::open_dialog(window, cx, move |dialog, _, _| {
            dialog
                .w(px(600.))
                .title("/r/a.rs")
                .child(div().w_full().h(px(500.)).child(shown.clone()))
        })
    });
    vcx.run_until_parked();
    vcx.executor().advance_clock(std::time::Duration::from_secs(1));
    vcx.run_until_parked();
    let first = vcx
        .debug_bounds("diff-viewer-code-0")
        .expect("control: line 1");
    let seventh = vcx
        .debug_bounds("diff-viewer-code-6")
        .expect("control: line 7");
    let left = v.read_with(vcx, |v, _| v.list().viewport_bounds().left());
    assert!(
        seventh.center().y > px(220.),
        "control: line 7 is under the text behind"
    );
    for x in [seventh.left() + px(1.), left - px(8.)] {
        vcx.simulate_mouse_down(
            point(first.left() + px(1.), first.center().y),
            MouseButton::Left,
            Modifiers::none(),
        );
        vcx.simulate_mouse_move(
            point(x, seventh.center().y),
            MouseButton::Left,
            Modifiers::none(),
        );
        vcx.simulate_mouse_up(
            point(x, seventh.center().y),
            MouseButton::Left,
            Modifiers::none(),
        );
        vcx.run_until_parked();
        let text = copied(vcx);
        if x > left {
            assert!(
                text.starts_with("fn line_1() {}\nfn line_2"),
                "control: inside: {text:?}"
            );
        } else {
            assert!(
                text.starts_with("fn line_1() {}\nfn line_2"),
                "the drag out of the viewer lost its selection: {text:?}"
            );
        }
    }
}
