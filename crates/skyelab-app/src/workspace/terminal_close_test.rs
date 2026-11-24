//! Acceptance: ⌘W closes the focused pane (the tab with its last one) and ⌘⇧D stacks a pane; closing a shell that
//! runs a command asks first while the setting is on, Cancel keeps it, and the setting off closes at once.

use super::terminal_panel::{CLOSE_TERMINAL_KEY, SPLIT_DOWN_KEY};
use super::thread_extras::prefs;
use super::threads_test::{add_rooted, test_backend};
use super::{NewTerminal, Workspace};
use crate::test_wait::wait_until;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;
use skyelab_core::terminal_layout::{Axis, PaneTree};

fn rooted(
    cx: &mut TestAppContext,
) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
    cx.executor().allow_parking(); // real shells
    cx.update(|cx| {
        gpui_component::init(cx);
        crate::settings::init(cx);
        skyelab_terminal::init(cx);
        super::init(cx);
    });
    let home = tempfile::tempdir().expect("tempdir");
    let folder = home.path().to_path_buf();
    let (ws, cx) = add_rooted(cx, |window, cx| {
        Workspace::new(
            folder,
            None,
            Default::default(),
            None,
            test_backend(),
            window,
            cx,
        )
    });
    (ws, cx, home)
}

fn first_tab(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<super::TerminalPanel> {
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let view = panel.read_with(cx, |p, _| p.panes[0].view.clone());
    view.update_in(cx, |v, window, cx| v.focus_handle(cx).focus(window, cx));
    panel
}

/// Types `sleep 30` into pane `at` once its shell is at the prompt, and waits until `sleep` runs in front of it (a
/// login shell's startup helpers count as busy too, so neither "busy" alone nor typing early proves it).
fn run_sleep(panel: &Entity<super::TerminalPanel>, at: usize, cx: &mut VisualTestContext) {
    let view = panel.read_with(cx, |p, _| p.panes[at].view.clone());
    let probe = view
        .read_with(cx, |v, _| v.terminal().probe())
        .expect("a live shell");
    wait_until(cx, "the prompt", |cx| {
        !probe.is_busy() && view.read_with(cx, |v, _| !v.terminal().text().trim().is_empty())
    });
    view.read_with(cx, |v, _| v.terminal().write(b"sleep 30\n".as_slice()));
    wait_until(cx, "sleep in front", |_| {
        probe.foreground().command.as_deref() == Some("sleep")
    });
}

fn no_confirm(cx: &mut VisualTestContext) {
    let off = skyelab_core::TerminalSettings {
        confirm_close: false,
    };
    cx.update(|_, cx| prefs::save_terminal(off, cx).detach());
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

#[gpui::test]
fn split_down_then_close_panes_and_the_tab(cx: &mut TestAppContext) {
    let (ws, cx, _home) = rooted(cx);
    // Pane and tab mechanics only (the confirm has its own tests): a login shell's prompt hooks can make an idle
    // shell look busy for a moment.
    no_confirm(cx);
    let panel = first_tab(&ws, cx);
    cx.simulate_keystrokes(SPLIT_DOWN_KEY);
    wait_until(cx, "a stacked pane", |cx| {
        panel.read_with(cx, |p, _| p.panes.len() == 2)
    });
    assert!(
        matches!(
            panel.read_with(cx, |p, _| p.tree.clone()),
            PaneTree::Split {
                axis: Axis::Column,
                ..
            }
        ),
        "⌘⇧D stacks"
    );
    cx.simulate_keystrokes(CLOSE_TERMINAL_KEY);
    cx.run_until_parked();
    assert!(!dialog_open(cx));
    assert_eq!(
        panel.read_with(cx, |p, _| p.panes.len()),
        1,
        "the focused pane closed"
    );
    assert!(matches!(
        panel.read_with(cx, |p, _| p.tree.clone()),
        PaneTree::Leaf(_)
    ));
    cx.simulate_keystrokes(CLOSE_TERMINAL_KEY);
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        0,
        "the last pane takes its tab"
    );
    assert!(
        ws.read_with(cx, |ws, _| ws.terminals_empty.is_some()),
        "the placeholder is back"
    );
}

#[gpui::test]
fn closing_a_busy_shell_asks_first(cx: &mut TestAppContext) {
    let (ws, cx, _home) = rooted(cx);
    let panel = first_tab(&ws, cx);
    run_sleep(&panel, 0, cx);
    let shell = panel.read_with(cx, |p, _| p.panes[0].view.clone());

    cx.simulate_keystrokes(CLOSE_TERMINAL_KEY);
    cx.run_until_parked();
    assert!(dialog_open(cx), "a running command asks first");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        1,
        "nothing closed yet"
    );
    // Cancel: the dialog goes, the tab and its process stay.
    cx.update(|window, cx| window.close_dialog(cx));
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        1,
        "Cancel keeps it"
    );
    assert!(shell.read_with(cx, |v, _| v.terminal().probe().is_some_and(|p| p.is_busy())));

    // The tab's × asks too; Close closes it.
    panel.update(cx, |_, cx| cx.emit(super::terminal_panel::TabEvent::Close));
    cx.run_until_parked();
    assert!(dialog_open(cx));
    ws.update_in(cx, |ws, window, cx| {
        ws.confirm_pending_terminal_close(window, cx)
    });
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.terminals.len()),
        0,
        "Close closes it"
    );
    assert!(!dialog_open(cx));
}

#[gpui::test]
fn with_the_setting_off_a_busy_shell_closes_at_once(cx: &mut TestAppContext) {
    let (ws, cx, _home) = rooted(cx);
    no_confirm(cx);
    let panel = first_tab(&ws, cx);
    run_sleep(&panel, 0, cx);
    cx.simulate_keystrokes(CLOSE_TERMINAL_KEY);
    cx.run_until_parked();
    assert!(!dialog_open(cx));
    assert_eq!(ws.read_with(cx, |ws, _| ws.terminals.len()), 0);
}
