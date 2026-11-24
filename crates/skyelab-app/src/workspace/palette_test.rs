//! The command palette inside a real workspace window (wrapped in `Root`, which hosts dialogs).

use super::threads_test::test_backend;
use super::*;
use crate::palette::{Entry, Kind, OPEN_PALETTE_KEY};
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::{Root, WindowExt as _};

pub(super) fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

/// A workspace in a `Root` window over `backend`, focused like `main` does it.
pub(super) fn open(
    backend: Backend,
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext) {
    let home = tempfile::tempdir().expect("tempdir").keep();
    let mut ws = None;
    let window = cx.add_window(|window, cx| {
        let view = cx.new(|cx| {
            Workspace::new(
                home,
                None,
                AccountsConfig::default(),
                None,
                backend,
                window,
                cx,
            )
        });
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
        ws = Some(view.clone());
        Root::new(view, window, cx)
    });
    let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
    cx.run_until_parked();
    (ws.expect("workspace built"), cx)
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

/// Opens the palette with its key, types `query` and presses enter.
fn choose(query: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();
    assert!(dialog_open(cx), "the palette key opens the dialog");
    cx.simulate_input(query);
    cx.run_until_parked();
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

fn entries(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<Entry> {
    ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
    })
}

fn labels(entries: &[Entry]) -> Vec<String> {
    entries.iter().map(|e| e.label.to_string()).collect()
}

fn settings_focused(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> bool {
    let settings = ws.read_with(cx, |ws, _| ws.settings.clone());
    let settings = settings.expect("the settings tab opened");
    cx.update(|window, cx| {
        settings
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
    })
}

#[gpui::test]
fn typing_and_enter_runs_the_action_in_the_workspace_and_hands_focus_back(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    choose("settings", cx);
    assert!(
        ws.read_with(cx, |ws, _| ws.settings_open),
        "the palette closed and Settings opened as a modal"
    );
    assert!(
        settings_focused(&ws, cx),
        "OpenSettings ran in the workspace and focused its tab"
    );
}

#[gpui::test]
fn escape_closes_and_restores_the_previous_focus(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();
    assert!(dialog_open(cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!dialog_open(cx), "escape closes the palette");
    let back = ws.update_in(cx, |ws, window, _| ws.focus.is_focused(window));
    assert!(back, "focus is back on the workspace");
}

#[gpui::test]
fn lists_only_named_skyelab_actions_with_their_bindings(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    let entries = entries(&ws, cx);
    let mut names = Vec::new();
    for e in &entries {
        if let Kind::Action(name) = &e.kind {
            assert!(name.starts_with("skyelab::"), "{name} is not ours");
            assert!(crate::palette::listed(name), "{name} is hidden");
            names.push(*name);
        }
    }
    for gone in [
        "skyelab::OpenCommandPalette",
        "skyelab::ConnectTyped",
        "zed::NoAction",
    ] {
        assert!(!names.contains(&gone), "{gone} listed");
    }
    let settings = entries
        .iter()
        .find(|e| matches!(e.kind, Kind::Action("skyelab::OpenSettings")))
        .expect("Open Settings listed");
    assert_eq!(settings.label.as_ref(), "Open Settings");
    assert!(settings.binding.is_some(), "its keybinding is shown");
}

#[gpui::test]
fn projects_threads_and_tabs_are_listed_and_run(cx: &mut TestAppContext) {
    setup(cx);
    let backend = test_backend();
    let dir = tempfile::tempdir().expect("tempdir").keep();
    let thread_id = {
        let store = lock(&backend.store).expect("store");
        let project = store.create_project("demo", &dir).expect("project");
        store
            .create_thread(&skyelab_store::NewThread {
                project_id: project.id,
                title: "Fix the flaky login test",
                cwd: &dir,
                account_id: "acme",
                status: "done",
            })
            .expect("thread")
            .id
    };
    let (ws, cx) = open(backend, cx);
    assert!(labels(&entries(&ws, cx)).contains(&"Switch to project: demo".to_owned()));

    choose("switch demo", cx);
    let selected = ws.read_with(cx, |ws, _| ws.project.as_ref().map(|p| p.name.clone()));
    assert_eq!(
        selected.as_deref(),
        Some("demo"),
        "the project entry selects it"
    );
    assert!(
        labels(&entries(&ws, cx)).contains(&"Open thread: Fix the flaky login test".to_owned())
    );

    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    choose("flaky login", cx);
    let opened = ws.read_with(cx, |ws, cx| {
        ws.open_threads
            .iter()
            .any(|t| t.read(cx).thread.as_ref().map(|t| t.id) == Some(thread_id))
    });
    assert!(opened, "the thread entry opens its page");

    // Settings is a page, not a tab: no "Go to tab" entry, and its own entry shows and focuses it again.
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    choose("open settings", cx);
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    assert!(!labels(&entries(&ws, cx)).contains(&"Go to tab: Settings".to_owned()));
    assert!(!settings_focused(&ws, cx));
    choose("open settings", cx);
    assert!(
        settings_focused(&ws, cx),
        "the Settings entry focuses the page"
    );
}

#[gpui::test]
fn go_to_tab_opens_the_closed_bottom_dock_and_focuses_the_terminal(cx: &mut TestAppContext) {
    cx.executor().allow_parking(); // a real shell PTY
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.new_terminal(&NewTerminal, window, cx)
    });
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.toggle_bottom(&ToggleBottomPanel, window, cx);
        ws.focus.focus(window, cx);
    });
    cx.run_until_parked();
    let bottom_open = |ws: &Entity<Workspace>, cx: &mut VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.dock.read(cx).is_dock_open(DockPlacement::Bottom)
        })
    };
    assert!(!bottom_open(&ws, cx), "precondition: bottom dock closed");

    // A tab is named after its folder while the shell sits at its prompt.
    let label = ws.read_with(cx, |ws, cx| ws.terminals[0].read(cx).tab_label());
    choose(&format!("tab {label}"), cx);
    assert!(bottom_open(&ws, cx), "Go to tab opens the terminal's dock");
    let terminal = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let focused = cx.update(|window, cx| {
        terminal
            .read(cx)
            .focus_handle(cx)
            .contains_focused(window, cx)
    });
    assert!(focused, "and focuses the terminal");
}

#[gpui::test]
fn new_thread_and_add_project_do_nothing_while_on_another_machine(cx: &mut TestAppContext) {
    use super::env_harness::*;
    let remote = remote_side(FakeTailnet::default());
    let (ws, _, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    let dir = tempfile::tempdir().expect("tempdir").keep();
    ws.update(cx, |ws, cx| ws.add_project(dir, cx));
    wait_until(cx, "local project", |cx| {
        ws.read_with(cx, |ws, _| !ws.projects.is_empty())
    });
    let id = ws.read_with(cx, |ws, _| ws.projects[0].id);
    cx.dispatch_action(SelectProject { id });
    cx.run_until_parked();
    connect(&ws, &remote, cx);

    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let local_tabs = ws.read_with(cx, |ws, _| ws.open_threads.len());
    assert_eq!(local_tabs, 0, "no local agent starts while remote");
}
