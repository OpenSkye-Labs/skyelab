//! Acceptance: ArrowUp in an empty prompt walks this thread's earlier prompts, ArrowDown walks back to empty, and
//! typed text is never replaced.

use crate::workspace::manage_harness::rig;
use crate::workspace::thread_model::Record;
use gpui::TestAppContext;

#[gpui::test]
fn up_and_down_walk_the_threads_prompts(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let id = r.thread("history", cx);
    {
        let mut s = r.f.store.lock().expect("lock");
        for p in [
            "first",
            "second\nline two",
            "second\nline two",
            "third\n\nAttached: a.png",
        ] {
            let (kind, payload) = Record::Prompt(p.into()).to_row();
            s.append_event(id, kind, &payload).expect("event");
        }
    }
    let view = r.open(id, cx);
    cx.run_until_parked();
    view.update_in(cx, |t, window, cx| t.focus_prompt(window, cx));
    let value = |cx: &mut gpui::VisualTestContext| {
        view.read_with(cx, |t, cx| t.prompt.read(cx).value().to_string())
    };

    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "third", "the newest, without its files line");
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "second\nline two", "repeats folded");
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "first");
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "first", "nothing older");

    cx.simulate_keystrokes("down");
    assert_eq!(value(cx), "second\nline two");
    // A recalled prompt opens with the caret at its start: Down first moves it to the last line.
    cx.simulate_keystrokes("down");
    assert_eq!(value(cx), "second\nline two");
    cx.simulate_keystrokes("down");
    assert_eq!(value(cx), "third");
    cx.simulate_keystrokes("down");
    assert_eq!(value(cx), "", "back to an empty prompt");

    cx.simulate_input("typed");
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "typed", "typed text stays");

    // With the slash menu open, Up belongs to the menu: the text stays.
    view.update_in(cx, |t, window, cx| {
        t.prompt.update(cx, |p, cx| p.set_value("", window, cx))
    });
    cx.simulate_input("/");
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |t, _| t.composer.menu.is_some()),
        "the menu opened"
    );
    cx.simulate_keystrokes("up");
    assert_eq!(value(cx), "/", "the history left it to the menu");
}
