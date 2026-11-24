//! A thread shown in two windows scrolls in each on its own: every window has its own list state, and rows added to
//! the thread still reach both.

use super::multi_window_test::{in_window, seed};
use super::thread_model::Record;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::{ListOffset, TestAppContext};

#[gpui::test]
fn each_window_scrolls_a_shared_thread_on_its_own(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    {
        let mut s = f.store.lock().expect("store");
        for n in 0..200 {
            let (kind, payload) = Record::Prompt(format!("message {n}")).to_row();
            s.append_event(id, kind, &payload).expect("event");
        }
    }
    let (a, cx) = open_workspace(&f, cx);
    let a_window = a.read_with(cx, |ws, _| ws.window_handle);
    cx.dispatch_action(NewWindow);
    cx.run_until_parked();
    let b_window = cx.update(|_, cx| window_handles(cx))[1];
    let b = cx.update(|_, cx| windows::workspaces(cx)[1].clone());
    in_window(a_window, &a, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    in_window(b_window, &b, cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| a.read(cx).open_view(id, cx))
        .expect("open in A");
    let rows = view.read_with(cx, |v, _| v.items.len());
    assert!(rows >= 200, "the thread loaded: {rows}");

    // A scrolls up to row 120; B still follows the end.
    view.read_with(cx, |v, _| {
        v.list_in(a_window).scroll_to(ListOffset {
            item_ix: 120,
            offset_in_item: gpui::px(0.),
        })
    });
    let state = view.read_with(cx, |v, _| {
        (
            v.list_in(a_window).logical_scroll_top().item_ix,
            v.list_in(b_window).is_following_tail(),
        )
    });
    assert_eq!(state, (120, true), "each window keeps its own scroll");

    // A new row reaches both windows' lists.
    view.update(cx, |v, cx| v.push(&Record::Prompt("one more".into()), cx));
    let counts = view.read_with(cx, |v, _| {
        (
            v.items.len(),
            v.list_in(a_window).item_count(),
            v.list_in(b_window).item_count(),
        )
    });
    assert_eq!(counts.1, counts.0, "A's list has every row");
    assert_eq!(counts.2, counts.0, "B's list has every row");

    // B closes: A's list is the one that stays, still where A left it.
    cx.update(|_, cx| {
        b_window
            .update(cx, |_, window, _| window.remove_window())
            .expect("B open")
    });
    cx.run_until_parked();
    let top = view.read_with(cx, |v, _| v.list_in(a_window).logical_scroll_top().item_ix);
    assert_eq!(top, 120);
    assert_eq!(
        view.read_with(cx, |v, _| v.lists.len()),
        1,
        "B's list went with it"
    );
}
