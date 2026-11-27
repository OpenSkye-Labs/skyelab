//! The composer's buttons answer an accessibility press (agent-desktop's `click`, AXPress) as a mouse click does:
//! GPUI's Click falls back to a click at the node's center, so a test clicks there.

use super::thread_model::Item;
use super::threads_test::{fixture, open_workspace, project_thread, setup};
use gpui::{TestAppContext, VisualTestContext};
use gpui_base::test_support::snapshots;

/// Clicks the center of the control named `name` (or starting with it), as GPUI's AccessKit Click does.
fn press(name: &str, cx: &mut VisualTestContext) {
    let snaps = cx.update(|window, _| snapshots(window));
    let node = snaps
        .iter()
        .find(|s| s.label().is_some_and(|l| l.starts_with(name)) && s.visible())
        .unwrap_or_else(|| {
            let names: Vec<_> = snaps.iter().filter_map(|s| s.label()).collect();
            panic!("no `{name}` on screen; named: {names:?}")
        });
    cx.simulate_click(node.bounds().center(), gpui::Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn send_answers_a_press(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    thread.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    cx.simulate_keystrokes("h i");
    cx.run_until_parked();
    press("Send (", cx);
    let sent = thread.read_with(cx, |t, _| {
        t.items
            .iter()
            .filter(|i| matches!(i, Item::Prompt(_)))
            .count()
    });
    assert_eq!(sent, 1, "the press sent the prompt");
    let left = thread.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string());
    assert_eq!(left, "");
    // The model chip opens its picker the same way.
    press("Model: ", cx);
    assert!(
        thread.read_with(cx, |t, _| t.composer.picker.is_some()),
        "the picker opened"
    );
}

/// An accessibility client may set the field's value instead of typing (AXValue): the input then emits no Change,
/// and Send must still know the prompt isn't empty.
#[gpui::test]
fn send_answers_a_press_after_the_value_is_set(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    let prompt = thread.read_with(cx, |t, _| t.prompt.clone());
    prompt.update_in(cx, |p, window, cx| p.set_value("hi", window, cx));
    cx.run_until_parked();
    press("Send (", cx);
    let sent = thread.read_with(cx, |t, _| {
        t.items
            .iter()
            .filter(|i| matches!(i, Item::Prompt(_)))
            .count()
    });
    assert_eq!(sent, 1, "the press sent the prompt");
}
