//! Acceptance: everything a thumb taps is at least 44 pt tall: thread rows, a turn's fold header, answer buttons.

use gpui::{TestAppContext, px, size};
use skyelab_mobile::{
    app::{Connect, MobileApp},
    source::{FakeSource, ThreadSource},
};

const MIN: f32 = 44.;

#[gpui::test]
fn tap_targets_are_44pt(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().to_path_buf();
    let connect: Connect =
        Box::new(|_: &str| Ok(Box::new(FakeSource::new()) as Box<dyn ThreadSource>));
    let (app, cx) = cx.add_window_view(|window, cx| MobileApp::new(data, connect, window, cx));
    let input = app.read_with(cx, |a, _| a.env_input.clone());
    input.update_in(cx, |s, window, cx| {
        s.set_value("laptop.tail1234.ts.net", window, cx)
    });
    app.update_in(cx, |a, window, cx| a.connect(window, cx));
    // A phone-sized window, drawn by the window itself so its debug bounds are recorded.
    cx.simulate_resize(size(px(390.), px(844.)));
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    draw(cx);
    let tall = |cx: &mut gpui::VisualTestContext, selector: &'static str| {
        let bounds = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is drawn"));
        assert!(
            bounds.size.height >= px(MIN),
            "{selector} is {:?} tall",
            bounds.size.height
        );
    };
    tall(cx, "thread-row");
    tall(cx, "back");

    // The demo's first thread has a folded turn and a waiting question, plan and permission.
    let first = app.read_with(cx, |a, _| a.threads()[0].id);
    app.update_in(cx, |a, window, cx| a.open_thread(first, window, cx));
    draw(cx);
    for selector in [
        "turn-fold",
        "answer-option",
        "answer-submit",
        "plan-approve",
        "allow-once",
        "send",
    ] {
        tall(cx, selector);
    }
}
