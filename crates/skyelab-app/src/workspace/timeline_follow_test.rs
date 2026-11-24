//! The timeline follows its end like a chat: an opened thread shows its latest rows, new rows keep it at the end,
//! and a user who scrolled up stays where they are until they scroll back down or send.

use super::multi_window_test::seed;
use super::thread_model::Record;
use super::threads_test::{fixture, open_workspace, setup};
use super::*;
use gpui::{ListOffset, TestAppContext};

#[gpui::test]
fn an_opened_thread_follows_its_end_until_the_user_scrolls_up(cx: &mut TestAppContext) {
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
    let (ws, cx) = open_workspace(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| ws.read(cx).open_view(id, cx))
        .expect("open");
    assert!(
        view.read_with(cx, |v, _| v.own_list().is_following_tail()),
        "an opened thread shows its end"
    );

    // The user scrolls up: a new row doesn't pull them down.
    view.read_with(cx, |v, _| {
        v.own_list().scroll_to(ListOffset {
            item_ix: 20,
            offset_in_item: gpui::px(0.),
        })
    });
    view.update(cx, |v, cx| v.push(&Record::Prompt("one more".into()), cx));
    cx.run_until_parked();
    let (top, following) = view.read_with(cx, |v, _| {
        (
            v.own_list().logical_scroll_top().item_ix,
            v.own_list().is_following_tail(),
        )
    });
    assert_eq!((top, following), (20, false), "a scrolled-up reader stays put");
}

#[gpui::test]
fn find_reveals_an_early_match_in_a_followed_thread(cx: &mut TestAppContext) {
    use super::thread_find::{FindHost as _, matches};
    use super::thread_items::ItemsView as _;
    setup(cx);
    let f = fixture(true);
    let id = seed(&f.store, &f.project_dir);
    {
        let mut s = f.store.lock().expect("store");
        for n in 0..200 {
            let text = if n == 3 { "the needle".to_string() } else { format!("message {n}") };
            let (kind, payload) = Record::Prompt(text).to_row();
            s.append_event(id, kind, &payload).expect("event");
        }
    }
    let (ws, cx) = open_workspace(&f, cx);
    ws.update_in(cx, |ws, window, cx| {
        ws.open_thread(&OpenThread { id }, window, cx)
    });
    cx.run_until_parked();
    let view = cx
        .update(|_, cx| ws.read(cx).open_view(id, cx))
        .expect("open");
    let ix = view.update(cx, |v, cx| {
        let found = matches(v.items(), "needle");
        let ix = found[0];
        let find = v.find_mut();
        find.matches = found;
        find.query = "needle".into();
        find.active = 0;
        v.reveal_match(cx);
        ix
    });
    cx.run_until_parked();
    let (top, following) = view.read_with(cx, |v, _| {
        (
            v.own_list().logical_scroll_top().item_ix,
            v.own_list().is_following_tail(),
        )
    });
    assert!(
        !following && top <= ix,
        "find left the match out of view: match row {ix}, top {top}, following {following}"
    );
}
