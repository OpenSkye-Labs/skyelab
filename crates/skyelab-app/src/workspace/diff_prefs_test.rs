//! Acceptance: the Changes tab's split / wrap / whitespace toggles are saved in settings.toml, and a new tab opens
//! with them.

use super::changes::ChangesView;
use super::thread_extras::prefs::{ThreadPrefs, get_diff};
use gpui::{AppContext as _, TestAppContext};

#[gpui::test]
fn diff_toggles_are_saved_and_reused(cx: &mut TestAppContext) {
    super::env_harness::setup(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = dir.path().join("settings.toml");
    cx.update(|cx| {
        cx.set_global(ThreadPrefs {
            path: Some(settings.clone()),
            settings: Default::default(),
            diff: Default::default(),
            terminal: Default::default(),
        })
    });
    let path = dir.path().to_path_buf();
    let (view, cx) = cx
        .add_window_view(|window, cx| ChangesView::new(path, None, "app".into(), None, window, cx));
    cx.run_until_parked();
    assert!(
        !view.read_with(cx, |v, _| v.prefs.split),
        "unified by default"
    );

    view.update(cx, |v, cx| v.set_prefs(|p| p.split = true, cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |v, _| v.prefs.split));
    assert!(
        cx.update(|_, cx| get_diff(cx)).split,
        "the app-wide setting follows"
    );
    let saved = skyelab_core::AppSettings::load(&settings).expect("saved");
    assert!(saved.diff.split, "written to settings.toml");

    let again = dir.path().to_path_buf();
    let second = cx.update(|window, cx| {
        cx.new(|cx| ChangesView::new(again, None, "b".into(), None, window, cx))
    });
    assert!(
        second.read_with(cx, |v, _| v.prefs.split),
        "a new tab starts split"
    );
}
