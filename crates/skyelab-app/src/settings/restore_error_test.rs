//! Acceptance: Restore defaults shows a failed save in the Settings error banner, as the other settings saves do, not
//! only in the log.

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
