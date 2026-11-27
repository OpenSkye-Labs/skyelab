//! Acceptance: the palette's submenu pages (T3's "Change theme", "New thread in…") and its Escape, in a real workspace.

use super::palette_test::{open, setup};
use super::threads_test::test_backend;
use super::*;
use crate::palette::OPEN_PALETTE_KEY;
use gpui::{TestAppContext, VisualTestContext};

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(crate::ui::has_active_dialog)
}

fn type_and(query: &str, key: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(query);
    cx.run_until_parked();
    cx.simulate_keystrokes(key);
    cx.run_until_parked();
}

fn theme(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| crate::ui::themes::get(cx).theme)
}

fn themed_setup(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| crate::ui::install_theme(cx).expect("themes load"));
}

#[gpui::test]
fn change_theme_opens_a_page_that_escape_and_backspace_leave(cx: &mut TestAppContext) {
    themed_setup(cx);
    let (_ws, cx) = open(test_backend(), cx);
    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();

    type_and("change theme", "enter", cx);
    assert!(dialog_open(cx), "a submenu row opens its page, not closes");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(dialog_open(cx), "Escape in a page goes back to the root");

    type_and("change theme", "enter", cx);
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert!(dialog_open(cx), "Backspace on an empty query goes back too");
    // Back at the root: an action the page doesn't list runs.
    type_and("toggle sidebar", "enter", cx);
    assert!(!dialog_open(cx), "the root's action ran and closed the palette");

    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();
    type_and("change theme", "enter", cx);
    assert_ne!(theme(cx), "dawn");
    type_and("dawn", "enter", cx);
    assert!(!dialog_open(cx), "picking in a page closes the palette");
    assert_eq!(theme(cx), "dawn", "and runs the item");
}

#[gpui::test]
fn escape_with_a_query_closes_at_once_and_gives_focus_back(cx: &mut TestAppContext) {
    setup(cx);
    let (ws, cx) = open(test_backend(), cx);
    cx.simulate_keystrokes(OPEN_PALETTE_KEY);
    cx.run_until_parked();
    type_and("sett", "escape", cx);
    assert!(!dialog_open(cx), "T3 closes on Escape, it doesn't clear first");
    let back = ws.update_in(cx, |ws, window, _| ws.focus.is_focused(window));
    assert!(back, "focus is back on the workspace");
}

#[gpui::test]
fn thread_entries_carry_their_last_use_for_recent_threads(cx: &mut TestAppContext) {
    setup(cx);
    let backend = test_backend();
    let dir = tempfile::tempdir().expect("tempdir").keep();
    let updated = {
        let store = lock(&backend.store).expect("store");
        let project = store.create_project("demo", &dir).expect("project");
        let t = store
            .create_thread(&skyelab_store::NewThread {
                project_id: project.id,
                title: "Recent one",
                cwd: &dir,
                account_id: "acme",
                status: "done",
            })
            .expect("thread");
        t.updated_at
    };
    let (ws, cx) = open(backend, cx);
    let entries = ws.update_in(cx, |ws, window, cx| {
        let focus = ws.focus.clone();
        ws.palette_entries(&focus, window, cx)
    });
    let thread = entries
        .iter()
        .find(|e| e.label.as_ref() == "Open thread: Recent one")
        .expect("the thread is listed");
    assert!(updated > 0);
    assert_eq!(thread.recency, updated, "ranked by when it was last used");
}
