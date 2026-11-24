//! Acceptance: dragging over an agent reply's text offers a floating "Quote" button; it adds the selection as a Quote
//! chip, sent as a markdown quote before the typed text. A plain click offers nothing.

use super::composer_context::ChipKind;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::{Modifiers, MouseButton, TestAppContext, VisualTestContext, point, px};

fn quote_shown(cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds("quote-selection").is_some()
}

#[gpui::test]
fn drag_select_quote_send(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    cx.run_until_parked();
    let reply = cx.debug_bounds("reply-text").expect("a reply drawn");
    let y = reply.top() + px(8.);

    // A press and release on the text without moving selects nothing.
    let at = point(reply.left() + px(4.), y);
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(!quote_shown(cx), "QUOTE_WITHOUT_SELECTION");

    // A drag across the first line selects it.
    let to = point(reply.left() + px(200.), y);
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert!(quote_shown(cx), "NO_QUOTE_BUTTON");

    let button = cx.debug_bounds("quote-selection").expect("quote button");
    cx.simulate_click(button.center(), Modifiers::none());
    cx.run_until_parked();
    let chip = thread
        .read_with(cx, |t, _| t.composer.contexts.first().cloned())
        .expect("QUOTE_NOT_ADDED");
    assert_eq!(chip.kind, ChipKind::Quote);
    assert!(!chip.text.trim().is_empty());
    assert!(
        "## Summary\n\nThe workspace has four crates".contains(chip.text.trim())
            || chip.text.contains("Summary"),
        "the selection is the reply's text: {:?}",
        chip.text
    );
    assert!(!quote_shown(cx), "the button goes once used");

    let sent = thread.read_with(cx, |t, _| t.with_contexts("why?", false));
    assert!(sent.starts_with("> "), "QUOTE_NOT_MARKDOWN: {sent}");
    assert!(sent.ends_with("\n\nwhy?"), "the quote goes first: {sent}");
    assert!(!sent.contains("```"), "a quote is not fenced: {sent}");
}

#[test]
fn quoted_prefixes_every_line() {
    assert_eq!(
        skyelab_core::terminal_context::quoted("a\n\nb\n"),
        "> a\n>\n> b"
    );
}
