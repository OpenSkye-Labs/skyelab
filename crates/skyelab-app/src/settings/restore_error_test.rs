//! Acceptance: Restore defaults shows a failed save in the Settings error banner, as the other settings saves do, not
//! only in the log; on General it also puts the update channel back to the build's own.

use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn a_failed_restore_shows_its_error(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    // settings.toml under a plain file: every save fails.
    let blocker = tmp.path().join("not-a-folder");
    std::fs::write(&blocker, "x").expect("write");
    let settings = blocker.join("settings.toml");
    cx.update(|cx| crate::ui::themes::load(Some(settings), cx));
    let path = tmp.path().join("cfg").join("accounts.toml");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(Some(path), AccountsConfig::default(), None, window, cx)
    });
    view.update_in(cx, |v, window, cx| {
        v.section = Section::Appearance;
        v.restore_defaults(window, cx);
    });
    cx.run_until_parked();
    let error = view.read_with(cx, |v, _| v.error.clone());
    assert!(
        error.as_deref().is_some_and(|e| e.contains("Appearance")),
        "the failed restore wasn't shown: {error:?}"
    );
}

#[gpui::test]
fn general_restore_puts_the_channel_back(cx: &mut TestAppContext) {
    use skyelab_core::update::Channel;
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let tmp = tempfile::tempdir().expect("tempdir");
    let settings = tmp.path().join("settings.toml");
    let fetch = crate::updates::fetcher(Some("v0.0.1".into()), None, true);
    cx.update(|cx| crate::updates::init(Some(settings.clone()), fetch, cx));
    cx.update(|cx| crate::updates::set_channel(Channel::Nightly, cx))
        .detach();
    cx.run_until_parked();
    let path = tmp.path().join("cfg").join("accounts.toml");
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(Some(path), AccountsConfig::default(), None, window, cx)
    });
    view.update_in(cx, |v, window, cx| {
        v.section = Section::General;
        v.restore_defaults(window, cx);
    });
    cx.run_until_parked();
    let build = Channel::of_build(env!("CARGO_PKG_VERSION"));
    assert_eq!(
        cx.update(|_, cx| cx.global::<crate::updates::Updates>().channel),
        build
    );
    let saved = skyelab_core::AppSettings::load(&settings)
        .expect("load")
        .updates;
    assert_eq!(saved.channel, None, "unset again, so it follows the build");
}
