//! Unlink pull request: the Details tab forgets a thread's PR without asking, the sidebar badge goes with it, and
//! the notice's Undo links it again.

use super::manage_harness::{Rig, rig};
use gpui::{SharedString, VisualTestContext};

fn badge(r: &Rig, id: i64, cx: &mut VisualTestContext) -> Option<SharedString> {
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    sidebar.read_with(cx, |s, _| {
        s.marks
            .get(&SharedString::from(format!("thread-{id}")))
            .and_then(|m| m.pr.clone())
    })
}

#[gpui::test]
fn unlink_a_pull_request_and_undo(cx: &mut gpui::TestAppContext) {
    let (r, cx) = rig(cx);
    let url = "https://github.com/acme/demo/pull/42";
    let id = r.thread("Fix it", cx);
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_pr_url(id, Some(url))
        .expect("link");
    r.reload(cx);
    let view = r.open(id, cx);
    assert_eq!(badge(&r, id, cx).as_deref(), Some("#42"));

    view.update(cx, |v, cx| v.unlink_pr(cx));
    // The write runs on the background executor, not inside the click on the UI thread.
    assert_eq!(
        r.row(id).and_then(|t| t.pr_url).as_deref(),
        Some(url),
        "WRITE_ON_UI_THREAD"
    );
    cx.run_until_parked();
    assert_eq!(
        r.row(id).and_then(|t| t.pr_url),
        None,
        "forgotten on the row"
    );
    assert_eq!(
        view.read_with(cx, |v, _| v.thread.as_ref().and_then(|t| t.pr_url.clone())),
        None
    );
    assert_eq!(badge(&r, id, cx), None, "the sidebar badge goes");
    assert!(
        r.notices(cx)
            .iter()
            .any(|n| n == "Unlinked pull request #42"),
        "an undo notice: {:?}",
        r.notices(cx)
    );

    // The notice's Undo.
    r.ws.update_in(cx, |ws, window, cx| {
        ws.relink_pr(id, url.into(), window, cx)
    });
    assert_eq!(r.row(id).and_then(|t| t.pr_url), None, "WRITE_ON_UI_THREAD");
    cx.run_until_parked();
    assert_eq!(r.row(id).and_then(|t| t.pr_url).as_deref(), Some(url));
    assert_eq!(
        badge(&r, id, cx).as_deref(),
        Some("#42"),
        "the badge is back"
    );
}
