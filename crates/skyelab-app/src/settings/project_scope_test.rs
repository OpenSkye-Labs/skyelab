//! Acceptance: with a project picked in "Applying settings for", Chat and Worktrees edit that project's own defaults,
//! the app-wide ones stay as they were, and Restore defaults sends the project back to inheriting.

use super::*;
use crate::workspace::thread_extras::prefs;
use gpui::TestAppContext;
use skyelab_core::thread_defaults::ThreadField;

#[gpui::test]
fn scoped_rows_edit_the_project(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        init(cx);
    });
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(&path, "[threads]\npermission_mode = \"acceptEdits\"\n").expect("write");
    cx.update(|cx| prefs::install(Some(path.clone()), cx));
    let work = dir.path().join("work");
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut s = Settings::new(None, AccountsConfig::default(), None, window, cx);
        s.projects = vec![("work".into(), work.clone())];
        s.show(Section::Chat, window, cx);
        s
    });
    cx.run_until_parked(); // draws the scope sentence over the rows

    view.update_in(cx, |s, window, cx| {
        s.set_scope(Some(work.clone()), window, cx);
        s.set_shown_field(ThreadField::PermissionMode, Some("plan".into()), cx);
    });
    let input = view.read_with(cx, |s, _| s.codex_model_input.clone());
    input.update_in(cx, |i, window, cx| i.set_value("gpt-5", window, cx));
    view.update(cx, |s, cx| s.save_codex_model(cx));
    view.update_in(cx, |s, window, cx| {
        s.show(Section::Worktrees, window, cx);
        s.set_threads(|t| t.set_project_worktree(&work, Some(true)), cx);
    });
    cx.run_until_parked();

    let now = cx.update(|_, cx| prefs::get(cx));
    assert_eq!(
        now.permission_mode.as_deref(),
        Some("acceptEdits"),
        "app-wide value untouched"
    );
    assert_eq!(now.codex_model, None);
    assert!(!now.new_worktree);
    let own = &now.projects[&work];
    assert_eq!(own.permission_mode.as_deref(), Some("plan"));
    assert_eq!(own.codex_model.as_deref(), Some("gpt-5"));
    assert_eq!(own.new_worktree, Some(true));
    assert!(saved(&path).projects.contains_key(&work));

    // Back to All projects: the field shows the app's (empty) Codex model again.
    view.update_in(cx, |s, window, cx| s.set_scope(None, window, cx));
    assert_eq!(input.read_with(cx, |i, _| i.value().to_string()), "");

    // Restore defaults on Worktrees clears only that page's override; on Chat the rest, and the table goes.
    view.update_in(cx, |s, window, cx| {
        s.set_scope(Some(work.clone()), window, cx);
        s.restore_defaults(window, cx);
    });
    let own = cx.update(|_, cx| prefs::get(cx)).projects[&work].clone();
    assert_eq!(own.new_worktree, None);
    assert_eq!(own.permission_mode.as_deref(), Some("plan"));
    view.update_in(cx, |s, window, cx| {
        s.show(Section::Chat, window, cx);
        s.restore_defaults(window, cx);
    });
    let now = cx.update(|_, cx| prefs::get(cx));
    assert!(now.projects.is_empty());
    assert_eq!(
        now.permission_mode.as_deref(),
        Some("acceptEdits"),
        "app-wide value untouched"
    );
    cx.run_until_parked();
    assert!(saved(&path).projects.is_empty());
}

fn saved(path: &std::path::Path) -> skyelab_core::ThreadSettings {
    skyelab_core::AppSettings::load(path).expect("load").threads
}
