//! Own UI kit, stage 6a (.agent/plans/2026-11-29-own-ui-kit-text-editing-6a.md), criterion 9: the caret blinks every
//! 500 ms while focused, holds still after 5 s idle, and a blink notifies only the input (a cached view beside it is
//! reused; docs/gpui/redraw.md: a blink must not rebuild the timeline's cached rows).

use super::text_input_harness::{field, focused, input};
use gpui::{TestAppContext, VisualTestContext};
use std::time::Duration;

fn visible(view: &gpui::Entity<super::text_input_harness::Field>, vcx: &mut VisualTestContext) -> bool {
    let field = input(view, vcx);
    field.read_with(vcx, |i, _| i.caret_visible())
}

fn wait(vcx: &mut VisualTestContext, ms: u64) {
    vcx.executor().advance_clock(Duration::from_millis(ms));
    vcx.run_until_parked();
}

#[gpui::test]
fn blinks_then_holds_and_only_redraws_itself(cx: &mut TestAppContext) {
    let (view, vcx) = focused(cx, 300., "abc");
    let renders = view.read_with(vcx, |v, cx| v.sibling.read(cx).renders.clone());
    renders.set(0);
    assert!(visible(&view, vcx), "shown when focus arrives");

    let mut seen = Vec::new();
    for _ in 0..9 {
        wait(vcx, 500);
        seen.push(visible(&view, vcx));
    }
    assert_eq!(
        seen,
        [false, true, false, true, false, true, false, true, false],
        "a 500 ms phase for 4.5 s"
    );
    assert_eq!(renders.get(), 0, "a cached view beside the field redrew while the caret blinked");

    // Past 5 s idle it holds, shown, and nothing ticks any more.
    wait(vcx, 1_000);
    let held = visible(&view, vcx);
    for _ in 0..6 {
        wait(vcx, 250);
        assert!(visible(&view, vcx), "held after 5 s idle");
    }
    assert!(held);

    // Typing restarts the blink.
    vcx.simulate_input("d");
    assert!(visible(&view, vcx), "solid while typing");
    let mut toggled = false;
    for _ in 0..6 {
        wait(vcx, 250);
        toggled |= !visible(&view, vcx);
    }
    assert!(toggled, "blinking again after typing");
}

#[gpui::test]
fn an_unfocused_field_has_no_caret_and_no_timer(cx: &mut TestAppContext) {
    let (view, vcx) = field(cx, 300., "abc");
    let renders = view.read_with(vcx, |v, _| v.renders.clone());
    *renders.borrow_mut() = 0;
    assert!(!visible(&view, vcx));
    let field = input(&view, vcx);
    assert!(field.read_with(vcx, |i, _| i.caret_bounds()).is_none(), "no caret drawn");
    wait(vcx, 2_000);
    assert_eq!(*renders.borrow(), 0, "nothing redraws an idle unfocused field");
}
