//! Acceptance: Settings → Threads, loaded from settings.toml the way the app starts, gives new threads their permission
//! mode, worktree box and model; a project's own pick still wins; saving writes the file.

use crate::workspace::thread_extras::prefs;
use crate::workspace::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use gpui::TestAppContext;
use skyelab_store::ModelChoice;

#[gpui::test]
fn new_threads_start_with_the_saved_defaults(cx: &mut TestAppContext) {
    setup(cx);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    std::fs::write(
        &path,
        "[threads]\npermission_mode = \"plan\"\nnew_worktree = true\nclaude_model = \"haiku\"\nclaude_effort = \"low\"\n",
    )
    .expect("write");
    cx.update(|cx| prefs::install(Some(path.clone()), cx));
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let haiku = ModelChoice {
        model: Some("haiku".into()),
        effort: Some("low".into()),
    };
    wait_until(cx, "the default model", |cx| {
        thread.read_with(cx, |t, _| t.composer.pick == haiku)
    });
    thread.read_with(cx, |t, _| {
        assert_eq!(t.mode, skyelab_agent::Mode::Plan);
        assert!(t.wt.use_worktree);
    });

    // The project picked a model before: that pick wins over the default.
    let project = thread.read_with(cx, |t, _| t.project_id);
    let sonnet = ModelChoice {
        model: Some("sonnet".into()),
        effort: None,
    };
    f.store
        .lock()
        .expect("lock")
        .set_project_model(project, &sonnet)
        .expect("pick");
    cx.dispatch_action(crate::workspace::NewThread);
    cx.run_until_parked();
    let second = ws
        .read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("tab");
    wait_until(cx, "the project's pick", |cx| {
        second.read_with(cx, |t, _| t.composer.pick == sonnet)
    });

    // Saving from Settings writes the file, keeping the rest of it.
    cx.update(|_, cx| prefs::edit(|t| t.generate_titles = false, cx))
        .detach();
    wait_until(cx, "the file saved", |_| {
        skyelab_core::AppSettings::load(&path).is_ok_and(|s| !s.threads.generate_titles)
    });
    let saved = skyelab_core::AppSettings::load(&path)
        .expect("load")
        .threads;
    assert_eq!(
        saved.permission_mode.as_deref(),
        Some("plan"),
        "the rest kept"
    );
}
