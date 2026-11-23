//! Acceptance (package D): the sidebar lists pinned threads first, archived ones apart, marks unread ones, and its
//! filter matches titles and message text.

use super::manage::ToggleArchivedThreads;
use super::manage_harness::rig;
use super::threads_test::wait_until;
use gpui::TestAppContext;

#[gpui::test]
fn pinned_first_archived_apart_unread_marked(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("alpha", cx);
    let b = r.thread("beta", cx);
    let c = r.thread("gamma", cx);
    {
        let s = r.f.store.lock().expect("lock");
        s.set_thread_pinned(a, true).expect("pin");
        s.set_thread_archived(b, true).expect("archive");
        s.set_thread_unread(c, true).expect("unread");
    }
    r.reload(cx);
    assert_eq!(
        r.listed(cx),
        ["alpha", "gamma"],
        "pinned first, archived out"
    );
    assert_eq!(r.archived(cx), ["beta"]);
    assert_eq!(r.marks(a, cx), (true, false));
    assert_eq!(r.marks(c, cx), (false, true));

    let open = |cx: &mut gpui::VisualTestContext| {
        let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
        sidebar.read_with(cx, |s, _| s.archived.as_ref().is_some_and(|a| a.open))
    };
    assert!(!open(cx), "archived threads start folded");
    cx.dispatch_action(ToggleArchivedThreads);
    cx.run_until_parked();
    assert!(open(cx));
}

#[gpui::test]
fn filter_matches_titles_and_message_text(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    r.thread("alpha", cx);
    let b = r.thread("beta", cx);
    r.f.store
        .lock()
        .expect("lock")
        .append_event(b, "prompt", "\"why does the zebra crash\"")
        .expect("append");
    let search =
        r.ws.read_with(cx, |ws, cx| ws.sidebar.read(cx).search.clone());
    // Typed like a user, so the input's change event drives the search.
    let filter = |text: &str, cx: &mut gpui::VisualTestContext| {
        search.update_in(cx, |s, window, cx| s.focus(window, cx));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a backspace"
        } else {
            "ctrl-a backspace"
        });
        if !text.is_empty() {
            cx.simulate_input(text);
        }
        cx.run_until_parked();
    };
    filter("zebra", cx);
    wait_until(cx, "the message match", |cx| r.listed(cx) == ["beta"]);
    filter("alp", cx);
    wait_until(cx, "the title match", |cx| r.listed(cx) == ["alpha"]);
    filter("nothing like it", cx);
    wait_until(cx, "no match", |cx| r.listed(cx).is_empty());
    filter("", cx);
    assert_eq!(r.listed(cx), ["beta", "alpha"]);
}
