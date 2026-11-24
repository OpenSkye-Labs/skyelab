//! The thread details card floats over the chat (T3's ⌘⇧B card): its toggle and Esc close it, a click outside
//! closes it only while it covers the chat column, and the right dock's "Open a surface" tab opens a surface by its
//! letter.

use super::focus_areas::FocusComposer;
use super::layout::{ToggleRightPanel, ToggleThreadDetails};
use super::threads_test::{fixture, open_workspace, project_thread, setup};
use super::*;
use gpui::{Modifiers, TestAppContext, point, px, size};

fn card(cx: &mut gpui::VisualTestContext) -> bool {
    cx.debug_bounds("thread-details-card").is_some()
}

#[gpui::test]
fn the_card_opens_on_its_toggle_and_closes_on_toggle_or_esc(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let _thread = project_thread(&f, &ws, cx);
    cx.run_until_parked();
    assert!(!card(cx), "closed at first");
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    assert!(card(cx), "the toggle opens it");
    assert!(ws.read_with(cx, |ws, cx| layout::docks_of(ws.window_handle, cx).details));
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    assert!(!card(cx), "the toggle closes it");
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    cx.dispatch_action(FocusComposer); // what Esc runs outside a text field
    cx.run_until_parked();
    assert!(!card(cx), "Esc closes it");
}

#[gpui::test]
fn a_click_outside_closes_the_card_only_where_it_covers_the_chat(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let _thread = project_thread(&f, &ws, cx);
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    let covers = |cx: &mut gpui::VisualTestContext| {
        ws.update_in(cx, |ws, window, cx| ws.card_covers_chat(window, cx))
    };
    let in_chat = point(px(320.), px(400.));
    // Wide enough that the card sits beside the chat column (T3's inline mode).
    cx.simulate_resize(size(px(2400.), px(1000.)));
    cx.run_until_parked();
    assert!(
        !covers(cx),
        "a wide window leaves the card room beside the chat"
    );
    cx.simulate_click(in_chat, Modifiers::none());
    cx.run_until_parked();
    assert!(
        card(cx),
        "INLINE_CLOSED: beside the chat column it stays open while you work"
    );
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    if !covers(cx) {
        cx.simulate_resize(size(px(1400.), px(1000.)));
        cx.run_until_parked();
    }
    assert!(covers(cx), "a narrow chat area puts the card over the chat");
    assert!(card(cx));
    cx.simulate_click(in_chat, Modifiers::none());
    cx.run_until_parked();
    assert!(!card(cx), "OUTSIDE_PRESS_KEPT: a click outside closes it");
}

#[gpui::test]
fn a_letter_in_open_a_surface_opens_that_surface(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let _thread = project_thread(&f, &ws, cx);
    cx.dispatch_action(ToggleRightPanel);
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.surfaces.read(cx).focus_handle(cx).focus(window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("d");
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.changes_views.len()),
        1,
        "D opened the thread's diff"
    );
}

#[gpui::test]
fn closing_the_card_from_inside_keeps_the_workspace_keys(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (ws, cx) = open_workspace(&f, cx);
    let _thread = project_thread(&f, &ws, cx);
    cx.simulate_keystrokes(layout::TOGGLE_DETAILS_KEY);
    cx.run_until_parked();
    ws.update_in(cx, |ws, window, cx| {
        ws.details.read(cx).focus_handle(cx).focus(window, cx)
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(layout::TOGGLE_DETAILS_KEY);
    cx.run_until_parked();
    assert!(!ws.read_with(cx, |ws, _| ws.details_open), "closed");
    cx.simulate_keystrokes(layout::TOGGLE_DETAILS_KEY);
    cx.run_until_parked();
    assert!(
        ws.read_with(cx, |ws, _| ws.details_open),
        "FOCUS_ORPHANED: the toggle key no longer reaches the workspace"
    );
}

#[gpui::test]
fn the_card_follows_pickers_another_window_made(cx: &mut TestAppContext) {
    use super::multi_window_test::{in_window, seed};
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    let (_orch_tx, orch_rx) = flume::unbounded();
    cx.update(|cx| set_hub(None, Some(orch_rx), cx));
    let (a, cx) = open_workspace(&f, cx);
    let a_window = a.read_with(cx, |ws, _| ws.window_handle);
    in_window(a_window, &a, cx, |ws, window, cx| ws.claim_hub(window, cx));
    cx.dispatch_action(OpenThread { id });
    cx.run_until_parked();
    in_window(a_window, &a, cx, |ws, _, cx| ws.set_details_open(true, cx));
    cx.run_until_parked();
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let b_window = cx.update(|_, cx| window_handles(cx))[1];
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    in_window(b_window, &b, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| a.read(cx).open_view(id, cx))
        .expect("open in A");
    let before = cx.update(|_, cx| a.read(cx).details.read(cx).renders);
    view.update(cx, |v, cx| {
        v.branch_picker.update(cx, |p, cx| p.set_open(true, cx))
    });
    cx.run_until_parked();
    let after = cx.update(|_, cx| a.read(cx).details.read(cx).renders);
    assert!(
        after > before,
        "STALE_PICKER: A's card never redrew for the picker B's attach made"
    );
}

#[gpui::test]
fn the_branch_row_opens_the_picker_ready_to_type(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    use super::changes_model_test::git;
    git(&f.project_dir, &["init", "-q", "-b", "main"]);
    git(&f.project_dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    cx.dispatch_action(ToggleThreadDetails);
    cx.run_until_parked();
    let row = cx.debug_bounds("details-branch").expect("branch row");
    cx.simulate_click(row.center(), Modifiers::none());
    cx.run_until_parked();
    let picker = thread.read_with(cx, |t, _| t.branch_picker.clone());
    assert!(picker.read_with(cx, |p, _| p.open), "the list is open");
    let query = picker.read_with(cx, |p, _| p.query.clone());
    assert!(
        cx.update(|window, cx| query.read(cx).focus_handle(cx).is_focused(window)),
        "typing goes to the search"
    );
    // Closed again, the hidden search gives the focus back, or the window's keys would reach nothing.
    picker.update(cx, |p, cx| p.set_open(false, cx));
    cx.run_until_parked();
    assert!(
        cx.update(|window, cx| !query.read(cx).focus_handle(cx).is_focused(window)
            && window.focused(cx).is_some()),
        "the focus went back"
    );
}
