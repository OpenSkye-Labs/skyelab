//! Acceptance: onboarding's motion runs only while a page enters. A page change crossfades in place (the old page
//! fades out over the new one behind a layer that takes the mouse, then goes) while the hero zooms in; it asks for
//! frames only during that, none once it settled, and none at all with Reduce Motion on (GPUI then draws the end state).

use super::onboarding::Step;
use super::onboarding_test::rig;
use gpui::{TestAppContext, VisualTestContext};
use std::time::Duration;

/// Frames the window asked for since the last draw (each runs the callbacks and draws again).
fn pending_frames(cx: &mut VisualTestContext) -> usize {
    let n = cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    n
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..40 {
        cx.executor().advance_clock(Duration::from_millis(16));
        if pending_frames(cx) == 0 {
            return;
        }
    }
    panic!("the page never settled");
}

#[gpui::test]
fn a_page_change_animates_then_goes_idle(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    settle(cx);
    rig.setup.update(cx, |o, cx| o.go(Step::Theme, cx));
    cx.run_until_parked();
    assert!(pending_frames(cx) > 0, "the new page slides in");
    settle(cx);
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(pending_frames(cx), 0, "an idle page asks for no frames");
}

#[gpui::test]
fn reduce_motion_changes_pages_without_frames(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    cx.update(|_, cx| cx.set_reduce_motion(true));
    settle(cx);
    rig.setup.update(cx, |o, cx| o.go(Step::Theme, cx));
    cx.run_until_parked();
    assert_eq!(pending_frames(cx), 0, "drawn at its end state at once");
    assert!(cx.debug_bounds("onboarding-leaving").is_none(), "no old page to fade out");
}

#[gpui::test]
fn a_step_change_crossfades_in_place_then_drops_the_old_page(cx: &mut TestAppContext) {
    let (rig, cx) = rig(cx);
    settle(cx);
    rig.setup.update(cx, |o, cx| o.go(Step::Theme, cx));
    cx.run_until_parked();
    let moving = cx.debug_bounds("onboarding-page").expect("new page drawn");
    let hero_in = cx.debug_bounds("onboarding-hero").expect("hero drawn");
    assert!(cx.debug_bounds("onboarding-leaving").is_some(), "the old page fades out over it");
    settle(cx);
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("onboarding-leaving").is_none(), "the old page is gone once faded");
    let settled = cx.debug_bounds("onboarding-page").expect("new page drawn");
    assert_eq!(moving.origin.x, settled.origin.x, "a crossfade, not a slide");
    let hero = cx.debug_bounds("onboarding-hero").expect("hero drawn");
    let zoom = hero_in.size.width / hero.size.width;
    assert!(zoom > 1.25, "the hero starts zoomed in: {zoom}");
    assert_eq!(pending_frames(cx), 0, "idle again");
}
