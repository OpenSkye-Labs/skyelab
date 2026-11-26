//! Acceptance (issue #11): a language the Mac can't recognize on the device asks before any audio goes to Apple, with
//! the microphone off. Cancel starts nothing and saves nothing; Allow saves that language and starts; the same language
//! then starts at once, another server-side one asks again, and an on-device one never asks.

use super::*;
use std::path::{Path, PathBuf};

/// Settings save to a real `settings.toml` (with something already in it) in `dir`.
fn settings_file(dir: &Path, cx: &mut VisualTestContext) -> PathBuf {
    let path = dir.join("settings.toml");
    std::fs::write(&path, "onboarded = true\n").expect("seed settings.toml");
    let p = path.clone();
    cx.update(|_, cx| prefs::install(Some(p), cx));
    path
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

fn mic(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(MIC_KEY);
    cx.run_until_parked();
}

/// The permission answer comes from another thread: wait for the ask, then draw it.
fn asked(cx: &mut VisualTestContext) {
    wait_until(cx, "the consent dialog", dialog_open);
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn starts(h: &Handles) -> usize {
    h.starts.load(Ordering::SeqCst)
}

fn click_allow(cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds("dictation-consent-allow")
        .expect("the dialog's Allow is drawn")
        .center();
    cx.simulate_click(at, gpui::Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_server_language_asks_first_and_cancel_starts_nothing(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = settings_file(tmp.path(), cx);
    let h = install(
        Fake {
            server: true,
            ..Fake::default()
        },
        cx,
    );
    *h.locale.lock().expect("locale") = "en-US".into();
    type_at(&thread, "draft", 5, cx);

    mic(cx);
    asked(cx);
    assert_eq!(starts(&h), 0, "MIC_ON_BEFORE_CONSENT");
    assert!(!listening(&thread, cx), "still dictating while asking");

    cx.update(|window, cx| window.close_dialog(cx)); // Cancel and Escape close it
    cx.run_until_parked();
    assert_eq!(starts(&h), 0, "CANCEL_STARTED_THE_MIC");
    assert_eq!(
        std::fs::read_to_string(&file).expect("settings.toml"),
        "onboarded = true\n",
        "CANCEL_WROTE_SETTINGS"
    );
    assert_eq!(prompt(&thread, cx).0, "draft");
    cx.simulate_keystrokes("!");
    assert_eq!(prompt(&thread, cx).0, "draft!", "PROMPT_LEFT_READ_ONLY");

    // Asked again next time: nothing was remembered.
    mic(cx);
    asked(cx);
    assert_eq!(starts(&h), 0);
}

#[gpui::test]
fn allow_saves_the_language_and_it_asks_once_per_language(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = settings_file(tmp.path(), cx);
    let h = install(
        Fake {
            server: true,
            ..Fake::default()
        },
        cx,
    );
    *h.locale.lock().expect("locale") = "en_US".into();
    type_at(&thread, "", 0, cx);

    mic(cx);
    asked(cx);
    click_allow(cx);
    wait_until(cx, "started after Allow", |_| h.live() == 1);
    assert_eq!(starts(&h), 1, "ALLOW_DIDNT_START");
    assert!(listening(&thread, cx));
    let saved = skyelab_core::AppSettings::load(&file).expect("settings.toml parses");
    assert!(saved.onboarded, "the rest of the file is kept");
    assert!(saved.allows_server_dictation("en-US"), "ALLOW_NOT_SAVED");

    // Finish, then dictate again in the same language: no second ask.
    mic(cx);
    h.say("hello", true, cx);
    assert_eq!(h.live(), 0);
    mic(cx);
    wait_until(cx, "started at once", |_| h.live() == 1);
    assert!(!dialog_open(cx), "ASKED_TWICE_FOR_ONE_LANGUAGE");
    assert_eq!(starts(&h), 2);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(h.live(), 0);

    // Another server-side language asks again, with the microphone off.
    *h.locale.lock().expect("locale") = "de-DE".into();
    mic(cx);
    asked(cx);
    assert_eq!(starts(&h), 2, "MIC_ON_FOR_NEW_LANGUAGE");
}

#[gpui::test]
fn an_on_device_language_never_asks(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = settings_file(tmp.path(), cx);
    let h = install(Fake::default(), cx);
    *h.locale.lock().expect("locale") = "ja-JP".into();
    type_at(&thread, "", 0, cx);
    mic(cx);
    wait_until(cx, "started", |_| h.live() == 1);
    assert!(!dialog_open(cx), "ON_DEVICE_ASKED");
    assert!(listening(&thread, cx));
    assert_eq!(
        std::fs::read_to_string(&file).expect("settings.toml"),
        "onboarded = true\n",
        "nothing saved for an on-device language"
    );
}

#[gpui::test]
fn a_session_that_turns_out_server_side_is_dropped_and_asks(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let h = install(
        Fake {
            server: true,
            probe_on_device: Some(true),
            ..Fake::default()
        },
        cx,
    );
    *h.locale.lock().expect("locale") = "pt-BR".into();
    type_at(&thread, "", 0, cx);
    mic(cx);
    asked(cx);
    assert_eq!(h.live(), 0, "UNAPPROVED_SERVER_SESSION_KEPT");
    assert!(!listening(&thread, cx));
}
