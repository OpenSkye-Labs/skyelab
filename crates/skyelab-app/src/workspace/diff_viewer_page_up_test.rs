//! Review proof: Page Up, or one big upward wheel turn, over wrapped rows never measured (after End) must not jump
//! over lines. The page is taller than the rows the list lays out above the view.

use super::changes_model::LineKind;
use super::diff_view::Line;
use super::diff_viewer::DiffViewer;
use crate::ui;
use gpui::{AppContext as _, Modifiers, TestAppContext, VisualTestContext, px};

const LINES: usize = 100;

fn drawn(vcx: &mut VisualTestContext) -> (usize, usize) {
    let all: Vec<usize> = (0..LINES)
        .filter(|ix| {
            let name: &'static str = Box::leak(format!("diff-viewer-code-{ix}").into_boxed_str());
            vcx.debug_bounds(name).is_some()
        })
        .collect();
    (
        *all.first().expect("rows are drawn"),
        *all.last().expect("rows are drawn"),
    )
}

fn wrapped_at_the_end(cx: &mut TestAppContext) -> (&mut VisualTestContext, (usize, usize)) {
    cx.update(|cx| {
        gpui_component::init(cx);
        ui::install_theme(cx).expect("the bundled themes parse");
        ui::bind_keys(cx);
        super::diff_viewer::bind_keys(cx);
    });
    let lines: Vec<Line> = (1..=LINES)
        .map(|i| (None, Some(i as u32), LineKind::Added, "x".repeat(600)))
        .collect();
    let v = cx.update(|cx| cx.new(|cx| DiffViewer::new("/r/notes.txt", lines, Vec::new(), cx)));
    let content = v;
    let (_, vcx) =
        cx.add_window_view(move |window, cx| ui::window_root(content.clone().into(), window, cx));
    vcx.simulate_resize(gpui::size(px(700.), px(1000.)));
    vcx.run_until_parked();
    let row = vcx
        .debug_bounds("diff-viewer-code-0")
        .expect("line 1 is there");
    vcx.simulate_click(row.center(), Modifiers::none());
    vcx.simulate_keystrokes("end");
    vcx.run_until_parked();
    let (first, last) = drawn(vcx);
    assert_eq!(last, LINES - 1, "End shows the last line");
    (vcx, (first, last))
}

#[gpui::test]
fn page_up_after_end_skips_no_wrapped_line(cx: &mut TestAppContext) {
    let (vcx, (first, _)) = wrapped_at_the_end(cx);
    vcx.simulate_keystrokes("pageup");
    vcx.run_until_parked();
    let (_, now_last) = drawn(vcx);
    assert!(
        now_last + 1 >= first,
        "Page Up skipped lines: index {first} was the first drawn, now the last drawn is {now_last}"
    );
}

#[gpui::test]
fn a_big_wheel_turn_up_after_end_skips_no_wrapped_line(cx: &mut TestAppContext) {
    let (vcx, (first, _)) = wrapped_at_the_end(cx);
    let at = vcx.debug_bounds("diff-viewer-code-99").expect("the last line").center();
    vcx.simulate_event(gpui::ScrollEvent {
        position: at,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(900.))),
        ..Default::default()
    });
    vcx.run_until_parked();
    let (_, now_last) = drawn(vcx);
    assert!(now_last + 1 >= first, "the wheel skipped lines: {first} then {now_last}");
}
