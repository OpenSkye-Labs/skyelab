//! Acceptance: Settings → Chat's send shortcut decides what Enter does in the composer (T3's three choices).
//! Shift-Enter always adds a line, Cmd/Ctrl-Enter always sends, and an empty prompt never sends.

use super::ThreadView;
use super::thread_extras::prefs;
use super::thread_model::Item;
use super::threads_test::{fixture, open_workspace, project_thread, setup};
use gpui::{Entity, TestAppContext, VisualTestContext};
use skyelab_core::SendShortcut;

const MOD_ENTER: &str = if cfg!(target_os = "macos") {
    "cmd-enter"
} else {
    "ctrl-enter"
};

/// Types `keys` into the focused composer, then checks it holds `left` and that `sent` prompts have gone, to the
/// agent or into the queue behind its running turn.
fn press(
    thread: &Entity<ThreadView>,
    keys: &str,
    left: &str,
    sent: usize,
    cx: &mut VisualTestContext,
) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    let value = thread.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string());
    assert_eq!(value, left, "the prompt box after {keys:?}");
    let gone = thread.read_with(cx, |t, _| {
        t.items
            .iter()
            .filter(|i| matches!(i, Item::Prompt(_)))
            .count()
            + t.composer.queue.iter().count()
    });
    assert_eq!(gone, sent, "prompts sent after {keys:?}");
}

/// Runs `check` on a fresh thread whose composer follows `shortcut`.
fn with_shortcut(
    shortcut: SendShortcut,
    cx: &mut TestAppContext,
    check: impl FnOnce(&Entity<ThreadView>, &mut VisualTestContext),
) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    cx.update(|_, cx| prefs::edit(move |t| t.send_shortcut = shortcut, cx))
        .detach();
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    check(&thread, cx);
}

#[gpui::test]
fn enter_sends_by_default(cx: &mut TestAppContext) {
    with_shortcut(SendShortcut::default(), cx, |thread, cx| {
        // An empty or blank prompt never sends.
        press(thread, "enter", "", 0, cx);
        press(thread, "space enter", " ", 0, cx);
        press(thread, "backspace h i enter", "", 1, cx);
        press(thread, "a shift-enter b", "a\nb", 1, cx);
        press(thread, "enter", "", 2, cx);
        press(thread, &format!("o k {MOD_ENTER}"), "", 3, cx);
    });
}

#[gpui::test]
fn mod_enter_for_multiline_sends_one_line_on_enter_only(cx: &mut TestAppContext) {
    with_shortcut(SendShortcut::ModEnterMultiline, cx, |thread, cx| {
        press(thread, "h i enter", "", 1, cx);
        press(thread, "a shift-enter b enter", "a\nb\n", 1, cx);
        press(thread, MOD_ENTER, "", 2, cx);
    });
}

#[gpui::test]
fn mod_enter_always_makes_enter_a_new_line(cx: &mut TestAppContext) {
    with_shortcut(SendShortcut::ModEnter, cx, |thread, cx| {
        press(thread, "h i enter", "hi\n", 0, cx);
        press(thread, "shift-enter", "hi\n\n", 0, cx);
        press(thread, MOD_ENTER, "", 1, cx);
    });
}
