//! Acceptance 4: in background mode SkyeLab stays up while a thread waits on an approval, and quits by itself once
//! nothing has worked or waited for the grace period.

use super::background::{self, IDLE_GRACE};
use super::background_keep_test::{approve, hidden_hub, quits, waiting_thread};
use super::manage_harness::rig;
use super::quit_guard::blockers;
use super::threads_test::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use std::time::Duration;

#[gpui::test]
fn background_quits_only_after_the_work_is_done(cx: &mut TestAppContext) {
    let (_r, vcx) = rig(cx);
    let view = waiting_thread(vcx);
    TestAppContext::update(vcx, background::enter);
    vcx.run_until_parked();
    let hub = hidden_hub(vcx).expect("the hidden hub");
    let mut hcx = VisualTestContext::from_window(hub, cx);

    hcx.executor().advance_clock(IDLE_GRACE * 3);
    hcx.run_until_parked();
    assert_eq!(quits(&mut hcx), 0, "a thread waits on an approval: it stays up");

    approve(&view, &mut hcx);
    wait_until(&mut hcx, "the turn to end", |cx| {
        cx.update(|_, cx| !blockers(cx).iter().any(|(_, b)| b.is_work()))
    });
    hcx.executor().advance_clock(Duration::from_secs(10));
    hcx.run_until_parked();
    assert_eq!(quits(&mut hcx), 0, "idle, but not for the whole grace yet");

    hcx.executor().advance_clock(IDLE_GRACE);
    hcx.run_until_parked();
    assert_eq!(quits(&mut hcx), 1, "idle past the grace: quit once");
}

#[gpui::test]
fn a_visible_window_ends_background_mode(cx: &mut TestAppContext) {
    let (_r, vcx) = rig(cx);
    TestAppContext::update(vcx, background::turn_on);
    assert!(TestAppContext::update(vcx, |cx| background::is_on(cx)));
    // A window shown (the user came back) ends it: idle time no longer quits.
    TestAppContext::update(vcx, background::window_shown);
    vcx.executor().advance_clock(IDLE_GRACE * 2);
    vcx.run_until_parked();
    assert!(!TestAppContext::update(vcx, |cx| background::is_on(cx)));
    assert_eq!(quits(vcx), 0, "no idle quit with a window open");
}
