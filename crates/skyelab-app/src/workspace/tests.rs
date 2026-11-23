use super::threads_test::test_backend;
use super::*;
use gpui::TestAppContext;
use skyelab_core::{Account, Binding};

fn setup(cx: &mut TestAppContext) {
    // Terminals run a real shell, whose PTY reader thread wakes the UI from outside the test scheduler.
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        init(cx);
    });
}

#[gpui::test]
fn keys_toggle_docks_open_settings_and_terminals(cx: &mut TestAppContext) {
    setup(cx);
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    cx.run_until_parked();
    let open = |ws: &Entity<Workspace>, at: DockPlacement, cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| ws.dock.read(cx).is_dock_open(at))
    };
    assert!(open(&ws, DockPlacement::Left, cx), "sidebar starts open");
    assert!(
        !open(&ws, DockPlacement::Bottom, cx),
        "bottom panel starts closed"
    );

    cx.simulate_keystrokes(TOGGLE_SIDEBAR_KEY);
    assert!(!open(&ws, DockPlacement::Left, cx));
    cx.simulate_keystrokes(TOGGLE_SIDEBAR_KEY);
    assert!(open(&ws, DockPlacement::Left, cx));

    cx.simulate_keystrokes(TOGGLE_BOTTOM_KEY);
    assert!(open(&ws, DockPlacement::Bottom, cx));
    cx.simulate_keystrokes(TOGGLE_BOTTOM_KEY);
    assert!(!open(&ws, DockPlacement::Bottom, cx));

    cx.simulate_keystrokes(OPEN_SETTINGS_KEY);
    cx.run_until_parked();
    let first = ws.read_with(cx, |ws, _| ws.settings.as_ref().and_then(|s| s.upgrade()));
    let first = first.expect("settings tab opened").entity_id();
    // A second press selects the existing tab instead of opening another.
    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(OPEN_SETTINGS_KEY);
    cx.run_until_parked();
    let second = ws.read_with(cx, |ws, _| ws.settings.as_ref().and_then(|s| s.upgrade()));
    assert_eq!(second.map(|s| s.entity_id()), Some(first));

    ws.update_in(cx, |ws, window, cx| ws.focus.focus(window, cx));
    cx.simulate_keystrokes(NEW_TERMINAL_KEY);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert_eq!(ws.terminals.len(), 1));
    assert!(
        open(&ws, DockPlacement::Bottom, cx),
        "a new terminal opens the bottom panel"
    );
}

#[test]
fn status_shows_the_bound_account_and_never_a_fallback() {
    let home = tempfile::tempdir().expect("tempdir");
    let mut config = AccountsConfig::default();
    assert_eq!(account_label(&config, home.path()), None);
    config
        .add_account(Account {
            id: "acme".into(),
            label: "Acme Team".into(),
            config_dir: home.path().join(".claude-acme"),
            provider: Default::default(),
            gh_config_dir: None,
            ssh_agent: false,
        })
        .expect("add account");
    assert_eq!(account_label(&config, home.path()), None, "unbound folder");
    config
        .add_binding(Binding::new(home.path().to_path_buf(), "acme".into()))
        .expect("bind");
    assert_eq!(
        account_label(&config, home.path()).as_deref(),
        Some("Acme Team")
    );
}

#[gpui::test]
fn shell_keys_reach_a_focused_terminal(cx: &mut TestAppContext) {
    setup(cx);
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        )
    });
    ws.update_in(cx, |ws, window, cx| {
        ws.new_terminal(&NewTerminal, window, cx)
    });
    cx.run_until_parked();
    // Readline and tmux need these; off macOS they are also the dock keys.
    cx.simulate_keystrokes("ctrl-b ctrl-j");
    ws.read_with(cx, |ws, cx| {
        let dock = ws.dock.read(cx);
        assert!(
            dock.is_dock_open(DockPlacement::Left),
            "ctrl-b stayed in the terminal"
        );
        assert!(
            dock.is_dock_open(DockPlacement::Bottom),
            "ctrl-j stayed in the terminal"
        );
    });
}

/// Inside Settings, the sidebar key still toggles the sidebar; Bind folder has its own key.
#[gpui::test]
fn cmd_b_toggles_sidebar_inside_settings(cx: &mut TestAppContext) {
    setup(cx);
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = cx.add_window_view(|window, cx| {
        let ws = Workspace::new(
            folder,
            None,
            AccountsConfig::default(),
            None,
            test_backend(),
            window,
            cx,
        );
        window.focus(&ws.focus, cx);
        ws
    });
    cx.simulate_keystrokes(OPEN_SETTINGS_KEY);
    cx.run_until_parked();
    let settings = ws
        .read_with(cx, |ws, _| ws.settings.as_ref().and_then(|s| s.upgrade()))
        .expect("settings open and focused");
    let sidebar = |cx: &mut gpui::VisualTestContext| {
        ws.read_with(cx, |ws, cx| {
            ws.dock.read(cx).is_dock_open(DockPlacement::Left)
        })
    };
    // With no account, Bind folder only sets an error, so its key is visible without a dialog.
    let bind_ran = |cx: &mut gpui::VisualTestContext| {
        settings.read_with(cx, |s, _| {
            s.error
                .as_deref()
                .is_some_and(|e| e.contains("add an account first"))
        })
    };
    assert!(sidebar(cx));
    cx.simulate_keystrokes(TOGGLE_SIDEBAR_KEY);
    assert!(!sidebar(cx), "the sidebar key toggles from inside Settings");
    assert!(!bind_ran(cx), "the sidebar key no longer runs Bind folder");
    cx.simulate_keystrokes(TOGGLE_SIDEBAR_KEY);
    assert!(sidebar(cx));
    cx.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-shift-b"
    } else {
        "ctrl-shift-b"
    });
    assert!(bind_ran(cx), "Bind folder moved to shift-b");
}
