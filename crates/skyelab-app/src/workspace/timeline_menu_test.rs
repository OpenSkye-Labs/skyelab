//! GPUI tests for right-click context menus on timeline messages (assistant replies & user prompts).
//!
//! Verifies with real `MouseButton::Right` clicks that the menu actually paints above the virtualized `list`
//! (non-empty bounds), copies raw markdown and plain text, quotes into the composer via `composer_quote.rs`, and
//! dismisses on Escape and backdrop click.

use super::thread::composer::composer_context::ChipKind;
use super::threads_test::{
    fixture, open_workspace, project_thread, setup, type_and_send, wait_done,
};
use gpui::{Modifiers, MouseButton, Point, Pixels, TestAppContext, VisualTestContext, point, px};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| {
        window.draw(cx).clear(cx);
        window.draw(cx).clear(cx);
    });
}

fn right_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    cx.simulate_mouse_move(at, None, Modifiers::default());
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    draw(cx);
}

fn assert_menu_painted(cx: &mut VisualTestContext) -> gpui::Bounds<Pixels> {
    let bounds = cx
        .debug_bounds("timeline-context-menu")
        .expect("timeline context menu painted");
    assert!(
        bounds.size.width > px(0.) && bounds.size.height > px(0.),
        "menu must paint with non-empty bounds: {bounds:?}"
    );
    bounds
}

#[gpui::test]
fn timeline_menu_right_click_reply_copies_and_quotes(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    draw(cx);

    let reply_at = cx
        .debug_bounds("reply-text")
        .expect("reply row drawn")
        .center();

    // 1. Copy message (raw markdown)
    right_click(cx, reply_at);
    assert!(thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    assert!(!thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    let raw = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .expect("raw markdown copied");
    assert!(raw.contains("## Summary"), "raw markdown expected: {raw:?}");

    // 2. Copy as plain text (markdown stripped via markdown_plain)
    right_click(cx, reply_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    assert!(!thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    let plain = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .expect("plain text copied");
    assert!(
        plain.contains("Summary") && !plain.contains("## "),
        "markdown syntax stripped: {plain:?}"
    );

    // 3. Quote into composer (reuses composer_quote.rs ChipKind::Quote path)
    right_click(cx, reply_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down down enter");
    draw(cx);
    assert!(!thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    let chip = thread
        .read_with(cx, |t, _| t.composer.contexts.first().cloned())
        .expect("quote chip added");
    assert_eq!(chip.kind, ChipKind::Quote);
    assert!(chip.text.contains("Summary"));
    let sent = thread.read_with(cx, |t, _| t.with_contexts("why?", false));
    assert!(sent.starts_with("> "), "markdown quote prefix: {sent}");
    assert!(sent.ends_with("\n\nwhy?"), "quote before text: {sent}");
}

#[gpui::test]
fn timeline_menu_right_click_prompt_copies_quotes_and_dismisses(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    draw(cx);

    let prompt_at = cx
        .debug_bounds("prompt-text")
        .expect("prompt bubble drawn")
        .center();

    // Dismiss with Escape
    right_click(cx, prompt_at);
    assert!(thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    assert_menu_painted(cx);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!thread.read_with(cx, |t, _| t.timeline.menu.is_open()));
    assert!(cx.debug_bounds("timeline-context-menu").is_none());

    // Dismiss with backdrop click outside the menu
    right_click(cx, prompt_at);
    let menu_bounds = assert_menu_painted(cx);
    let outside = point(px(10.), px(10.));
    assert!(!menu_bounds.contains(&outside));
    cx.simulate_mouse_down(outside, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::default());
    draw(cx);
    assert!(!thread.read_with(cx, |t, _| t.timeline.menu.is_open()));

    // Copy message on prompt
    right_click(cx, prompt_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down enter");
    draw(cx);
    let copied = cx
        .read_from_clipboard()
        .and_then(|c| c.text())
        .expect("prompt copied");
    assert_eq!(copied, "hi\nthere");

    // Quote into composer on prompt (2nd item, no "Copy as plain text" on plain prompts)
    right_click(cx, prompt_at);
    assert_menu_painted(cx);
    cx.simulate_keystrokes("down down enter");
    draw(cx);
    let chip = thread
        .read_with(cx, |t, _| t.composer.contexts.first().cloned())
        .expect("prompt quote chip added");
    assert_eq!(chip.kind, ChipKind::Quote);
    assert_eq!(chip.text, "hi\nthere");
}
