//! A Settings save applies its one change to settings.toml as it is on disk: a project table edited by hand while the
//! app runs survives the next Chat or Worktrees change.

use super::*;
use crate::workspace::thread_extras::prefs;
use gpui::TestAppContext;
use skyelab_core::thread_defaults::ThreadField;
use std::path::Path;

#[gpui::test]
fn a_save_keeps_a_hand_edited_project_table(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[threads]\n").expect("write");
    cx.update(|cx| prefs::install(Some(path.clone()), cx));
    let (view, cx) = cx.add_window_view(|window, cx| {
        Settings::new(None, AccountsConfig::default(), None, window, cx)
    });

    // Edited by hand after the app loaded the file.
    std::fs::write(
        &path,
        "[threads.projects.\"/other\"]\npermission_mode = \"plan\"\n",
    )
    .expect("hand edit");
    view.update(cx, |s, cx| {
        s.set_shown_field(ThreadField::ClaudeModel, Some("opus".into()), cx)
    });
    cx.run_until_parked();

    let saved = skyelab_core::AppSettings::load(&path)
        .expect("load")
        .threads;
    assert_eq!(saved.claude_model.as_deref(), Some("opus"));
    assert_eq!(
        saved.projects[Path::new("/other")]
            .permission_mode
            .as_deref(),
        Some("plan"),
        "the hand-edited table survives"
    );
}
