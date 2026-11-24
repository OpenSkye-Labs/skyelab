//! Acceptance (package D): a turn that finishes in a thread nobody is looking at marks it unread and notifies;
//! looking at it clears the marker. A background window gets an OS notification.

use super::manage_harness::rig;
use super::thread::ThreadViewEvent;
use gpui::TestAppContext;

#[gpui::test]
fn unseen_turns_mark_unread_and_notify(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_app_identity("dev.openskye.skyelab", "SkyeLab"));
    let (r, cx) = rig(cx);
    let a = r.thread("first", cx);
    let b = r.thread("second", cx);
    let view_a = r.open(a, cx);
    r.open(b, cx); // b is on screen now, a is a background tab

    view_a.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    assert!(
        r.row(a).expect("row").unread,
        "stored, so it survives a restart"
    );
    assert_eq!(r.marks(a, cx), (false, true));
    assert_eq!(r.notices(cx), [format!("thread {a}")], "an in-app toast");

    r.open(a, cx);
    assert!(!r.row(a).expect("row").unread, "seen");
    assert_eq!(r.marks(a, cx), (false, false));

    // The window in the background: even the shown thread is unseen, and the OS is told.
    cx.deactivate_window();
    cx.run_until_parked();
    view_a.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    assert!(r.row(a).expect("row").unread);
    let shown = cx.shown_system_notifications();
    assert!(
        shown
            .iter()
            .any(|n| n.title.as_ref() == "Thread completed" && n.body.as_ref() == "first"),
        "{shown:?}"
    );
}

#[gpui::test]
fn the_shown_thread_and_archived_threads_stay_quiet(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("watched", cx);
    let view = r.open(a, cx);
    view.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    assert!(!r.row(a).expect("row").unread, "the user saw it finish");
    assert!(r.notices(cx).is_empty());

    let b = r.thread("archived", cx);
    let view_b = r.open(b, cx);
    view_b.update(cx, |v, _| {
        if let Some(t) = v.thread.as_mut() {
            t.archived_at = Some(1);
        }
    });
    r.open(a, cx);
    view_b.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    assert!(r.notices(cx).is_empty(), "archived threads never notify");
}

/// A Compare page shows every thread of its group: one finishing there, in the focused window, is seen, and the
/// page coming back to the focused window clears a mark left while it was in the background.
#[gpui::test]
fn every_thread_on_a_compare_page_is_seen(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let a = r.thread("opus", cx);
    let b = r.thread("sonnet", cx);
    let views = [r.open(a, cx), r.open(b, cx)];
    for v in &views {
        v.update(cx, |v, _| {
            if let Some(t) = v.thread.as_mut() {
                t.compare_group = Some(7);
            }
        });
    }
    r.ws.update_in(cx, |ws, window, cx| {
        ws.show(super::center::Page::Compare(7), window, cx)
    });
    cx.run_until_parked();
    for v in &views {
        v.update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    }
    cx.run_until_parked();
    for id in [a, b] {
        assert!(!r.row(id).expect("row").unread, "thread {id} was on screen");
    }
    assert!(r.notices(cx).is_empty());
    // A sibling started in the background beside the page (as a multi-model send starts them) is on screen too.
    views[1].update(cx, |v, cx| {
        v.background = true;
        let t = v.thread.clone().expect("thread");
        cx.emit(ThreadViewEvent::Started(Box::new(t)));
    });
    cx.run_until_parked();
    assert!(!r.row(b).expect("row").unread, "started on screen");

    cx.deactivate_window();
    cx.run_until_parked();
    views[0].update(cx, |_, cx| cx.emit(ThreadViewEvent::TurnDone));
    cx.run_until_parked();
    assert!(r.row(a).expect("row").unread, "the window was in the background");
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(!r.row(a).expect("row").unread, "seen once the window is back");
}
