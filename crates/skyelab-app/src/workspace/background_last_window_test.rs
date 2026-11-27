//! Acceptance 6 (the path off macOS, driven here on any OS): closing the last window while a thread works asks with
//! "Keep running in background"; with the setting on it keeps running without asking; with nothing working it
//! closes (and the app quits, as GPUI does off macOS).

use super::background;
use super::background_keep_test::waiting_thread;
use super::manage_harness::rig;
use super::quit_guard::{blockers, can_keep, may_close_on, set_keeps};
use gpui::TestAppContext;

#[gpui::test]
fn nothing_working_closes_without_asking(cx: &mut TestAppContext) {
    let (_r, vcx) = rig(cx);
    let close = vcx.update(|window, cx| may_close_on(false, window, cx));
    assert!(close, "nothing in flight: the window closes");
    assert!(!vcx.update(crate::ui::has_active_dialog));
    assert!(!TestAppContext::update(vcx, |cx| background::is_on(cx)));
}

#[gpui::test]
fn closing_the_last_window_offers_keep_running(cx: &mut TestAppContext) {
    let (_r, vcx) = rig(cx);
    waiting_thread(vcx);
    assert!(vcx.update(|_, cx| can_keep(&blockers(cx))));

    // Always keep running (Settings): the window closes without asking and SkyeLab stays up.
    vcx.update(|_, cx| set_keeps(true, cx)).expect("saved");
    let close = vcx.update(|window, cx| may_close_on(false, window, cx));
    assert!(close, "the window closes without asking");
    assert!(!vcx.update(crate::ui::has_active_dialog));
    assert!(
        TestAppContext::update(vcx, |cx| background::is_on(cx)),
        "and SkyeLab keeps running in background"
    );

    // Asked each time (the default): the window stays while the dialog offers Keep running.
    vcx.update(|_, cx| set_keeps(false, cx)).expect("saved");
    TestAppContext::update(vcx, background::window_shown);
    let close = vcx.update(|window, cx| may_close_on(false, window, cx));
    vcx.run_until_parked();
    assert!(!close, "a waiting thread: the window stays while it asks");
    assert!(vcx.update(crate::ui::has_active_dialog));
    assert!(vcx.debug_bounds("quit-keep").is_some(), "Keep running is offered");
    assert!(!TestAppContext::update(vcx, |cx| background::is_on(cx)), "not yet");
}
