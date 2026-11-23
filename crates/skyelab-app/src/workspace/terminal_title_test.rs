//! Acceptance: a terminal tab is titled by the command running in front of its shell, else its folder's name, and
//! a rename (double-click on the title) sticks: Enter keeps it, Esc cancels, empty goes back to automatic.

use super::NewTerminal;
use super::terminal_panel::TerminalPanel;
use super::worktree_thread_test::{fixture, open_thread, wait_until};
use gpui::{Entity, Focusable as _, VisualTestContext};

fn label_after_poll(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> String {
    panel.update(cx, |p, cx| p.poll(cx).detach());
    cx.run_until_parked();
    panel.read_with(cx, |p, _| p.tab_label().to_string())
}

fn rename(panel: &Entity<TerminalPanel>, typed: &str, end: &str, cx: &mut VisualTestContext) {
    panel.update_in(cx, |p, window, cx| p.start_rename(window, cx));
    cx.run_until_parked();
    assert!(
        panel.read_with(cx, |p, _| p.renaming.is_some()),
        "the field opened"
    );
    if typed.is_empty() {
        cx.simulate_keystrokes("backspace");
    } else {
        cx.simulate_input(typed);
    }
    cx.simulate_keystrokes(end);
    cx.run_until_parked();
    assert!(
        panel.read_with(cx, |p, _| p.renaming.is_none()),
        "{end} ends the rename"
    );
}

#[gpui::test]
fn the_title_follows_the_command_and_a_rename_sticks(cx: &mut gpui::TestAppContext) {
    let f = fixture();
    let (ws, _thread, cx) = open_thread(&f, cx);
    cx.dispatch_action(NewTerminal);
    wait_until(cx, "a terminal", |cx| {
        ws.read_with(cx, |ws, _| ws.terminals.len() == 1)
    });
    let panel = ws.read_with(cx, |ws, _| ws.terminals[0].clone());
    let view = panel.read_with(cx, |p, _| p.panes[0].view.clone());
    view.update_in(cx, |v, window, cx| v.focus_handle(cx).focus(window, cx));
    let folder = f
        .project
        .file_name()
        .expect("named")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        label_after_poll(&panel, cx),
        folder,
        "at the prompt: the folder"
    );

    view.read_with(cx, |v, _| v.terminal().write(b"sleep 30\n".as_slice()));
    wait_until(cx, "sleep in the title", |cx| {
        label_after_poll(&panel, cx) == "sleep"
    });

    rename(&panel, "Dev server", "enter", cx);
    assert_eq!(label_after_poll(&panel, cx), "Dev server");
    assert_eq!(
        panel.read_with(cx, |p, _| p.saved().name),
        Some("Dev server".into()),
        "the name is saved with the tab"
    );
    rename(&panel, "Other", "escape", cx);
    assert_eq!(
        label_after_poll(&panel, cx),
        "Dev server",
        "Esc keeps the old name"
    );
    rename(&panel, "", "enter", cx);
    assert_eq!(
        label_after_poll(&panel, cx),
        "sleep",
        "empty goes back to automatic"
    );
    view.read_with(cx, |v, _| v.terminal().shutdown());
}
