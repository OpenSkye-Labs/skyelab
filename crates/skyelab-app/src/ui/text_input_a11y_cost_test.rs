use super::text_input_a11y::runs_of;
use super::text_input_harness::{focused, input};
use gpui::TestAppContext;
use std::time::{Duration, Instant};

#[gpui::test]
fn text_runs_cost_grows_with_the_text_not_its_square(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 200., "");
    let field = input(&view, vcx);
    let mut cost = |n: usize| -> Duration {
        field.update_in(vcx, |i, window, cx| i.set_value("a".repeat(n), window, cx));
        vcx.run_until_parked();
        field.read_with(vcx, |i, _| {
            assert!(i.range_bounds(0..1).is_some(), "not drawn");
            // The best of five, so a busy machine doesn't decide it.
            let timed = (0..5).map(|_| {
                let start = Instant::now();
                std::hint::black_box(runs_of(i, 1.));
                start.elapsed()
            });
            timed.min().expect("five runs")
        })
    };
    let (small, big) = (cost(4_000), cost(32_000));
    assert!(
        big < small * 24,
        "8x the text took {big:?} against {small:?}"
    );
}
