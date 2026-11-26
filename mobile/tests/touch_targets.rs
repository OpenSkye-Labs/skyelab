//! Acceptance: everything a thumb taps is at least 44 pt tall: thread rows, shelf fold headers, shelf/snooze
//! actions, a turn's fold header, and answer buttons.

use gpui::{TestAppContext, px, size};
use skyelab_mobile::{
    app::{Connect, MobileApp},
    source::{FakeSource, ThreadSource},
    thread_rows::now_ms,
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
    tall(cx, "shelf-toggle");
    tall(cx, "back");

    // Expand shelf actions on the first thread: Settle and snooze preset buttons must be >= 44 pt.
    let ids: Vec<i64> = app.read_with(cx, |a, _| a.threads().iter().map(|t| t.id).collect());
    app.update(cx, |a, cx| a.toggle_shelf_actions(ids[0], cx));
    draw(cx);
    tall(cx, "shelf-settle");
    tall(cx, "shelf-snooze");

    // Put idle threads on the Snoozed and Settled shelves and verify fold headers, Wake, and Restore >= 44 pt.
    app.update(cx, |a, cx| {
        a.snooze_thread(ids[1], now_ms() + 3_600_000, cx);
        a.settle_thread(ids[2], cx);
    });
    draw(cx);
    tall(cx, "shelf-fold");
    app.update(cx, |a, cx| {
        a.toggle_snoozed_shelf(cx);
        a.toggle_settled_shelf(cx);
    });
    draw(cx);
    tall(cx, "shelf-wake");
    tall(cx, "shelf-restore");

    // The demo's first thread has a folded turn and a waiting question, plan and permission.
    let first = ids[0];
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
