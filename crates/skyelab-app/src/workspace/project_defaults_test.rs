//! Acceptance: a project's own new-thread defaults in settings.toml beat the app-wide ones for that project only, with
//! nothing set a new thread starts in Full access, an explicit Ask stays Ask, and the project's menu opens Settings
//! scoped to it.

use crate::workspace::manage::{ManageProject, ProjectOp};
use crate::workspace::thread::ThreadView;
use crate::workspace::thread_extras::prefs;
use crate::workspace::threads_test::{fixture, open_workspace, project_thread, setup, wait_until};
use crate::workspace::{NewThread, Workspace};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_agent::Mode;
use skyelab_store::ModelChoice;

fn new_thread(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<ThreadView> {
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| ws.open_threads.last().cloned())
        .expect("tab")
}

fn choice(model: &str, effort: &str) -> ModelChoice {
    ModelChoice {
        model: Some(model.into()),
        effort: Some(effort.into()),
    }
}

#[gpui::test]
fn project_overrides_apply_to_its_new_threads(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    let demo = f.project_dir.display().to_string();
    std::fs::write(
        &path,
        format!(
            "[threads]\nclaude_model = \"haiku\"\nclaude_effort = \"low\"\n\n\
             [threads.projects.\"{demo}\"]\npermission_mode = \"plan\"\nclaude_model = \"opus\"\nnew_worktree = true\n"
        ),
    )
    .expect("write");
    cx.update(|cx| prefs::install(Some(path.clone()), cx));
    let (ws, cx) = open_workspace(&f, cx);

    // The project's own defaults, its unset effort from the app's.
    let thread = project_thread(&f, &ws, cx);
    wait_until(cx, "the project's model", |cx| {
        thread.read_with(cx, |t, _| t.composer.pick == choice("opus", "low"))
    });
    thread.read_with(cx, |t, _| {
        assert_eq!(t.mode, Mode::Plan);
        assert!(t.wt.use_worktree);
    });

    // Full access as a project's mode is kept.
    let mut next = cx.update(|_, cx| prefs::get(cx));
    let key = f.project_dir.clone();
    next.projects.get_mut(&key).expect("demo").permission_mode = Some(Mode::Bypass.as_str().into());
    let all = next.clone();
    cx.update(|_, cx| prefs::edit(move |t| *t = all.clone(), cx))
        .detach();
    let second = new_thread(&ws, cx);
    second.read_with(cx, |t, _| assert_eq!(t.mode, Mode::Bypass));

    // Overrides for another folder leave this project on the app defaults.
    let theirs = next.projects.remove(&key).expect("demo");
    next.projects.insert("/elsewhere".into(), theirs);
    cx.update(|_, cx| prefs::edit(move |t| *t = next.clone(), cx))
        .detach();
    let third = new_thread(&ws, cx);
    wait_until(cx, "the app default model", |cx| {
        third.read_with(cx, |t, _| t.composer.pick == choice("haiku", "low"))
    });
    // Nothing set for this project or the app: Full access, as T3.
    third.read_with(cx, |t, _| {
        assert_eq!(t.mode, Mode::Bypass);
        assert!(!t.wt.use_worktree);
    });

    // An explicit app-wide Ask stays Ask.
    cx.update(|_, cx| {
        prefs::edit(
            |t| t.permission_mode = Some(Mode::Default.as_str().into()),
            cx,
        )
    })
    .detach();
    let fourth = new_thread(&ws, cx);
    fourth.read_with(cx, |t, _| assert_eq!(t.mode, Mode::Default));
    wait_until(cx, "the file saved", |_| {
        skyelab_core::AppSettings::load(&path).is_ok_and(|s| {
            s.threads
                .projects
                .contains_key(std::path::Path::new("/elsewhere"))
        })
    });
}

#[gpui::test]
fn project_menu_opens_scoped_settings(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let _ = project_thread(&f, &ws, cx);
    let id = ws.read_with(cx, |ws, _| ws.projects[0].id);
    cx.dispatch_action(ManageProject {
        id,
        op: ProjectOp::Settings,
    });
    cx.run_until_parked();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.settings_open);
        let s = ws.settings.as_ref().expect("settings").read(cx);
        assert_eq!(s.section, crate::settings::Section::Chat);
        assert_eq!(s.scope.as_deref(), Some(f.project_dir.as_path()));
        assert!(
            s.projects.iter().any(|(_, p)| *p == f.project_dir),
            "the scope menu lists the project"
        );
    });
}
