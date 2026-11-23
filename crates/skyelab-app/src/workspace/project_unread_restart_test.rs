//! An unread thread stays marked in the flat list (its row says Done) with no view open, as after a restart, even
//! while another project is selected.

use super::manage_harness::rig;
use super::*;
use gpui::TestAppContext;

#[gpui::test]
fn an_unread_thread_stays_marked_without_an_open_view(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    let first = r.project;
    let id = r.thread("finished while away", cx);
    // The stored flag a finished unseen turn or a background start leaves; it outlives the views.
    r.f.store
        .lock()
        .expect("lock")
        .set_thread_unread(id, true)
        .expect("flag");
    r.reload(cx);
    assert!(
        r.ws.read_with(cx, |ws, cx| ws.open_view(id, cx).is_none()),
        "precondition: no view, as after a restart"
    );
    let other = r.f.project_dir.parent().expect("parent").join("other");
    std::fs::create_dir_all(&other).expect("other dir");
    r.ws.update(cx, |ws, cx| ws.add_project(other, cx));
    super::threads_test::wait_until(cx, "the other project selected", |cx| {
        r.ws.read_with(cx, |ws, _| {
            ws.project.as_ref().is_some_and(|p| p.id != first)
        })
    });
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    let marked = sidebar.read_with(cx, |s, _| {
        s.marks
            .get(&SharedString::from(format!("thread-{id}")))
            .is_some_and(|m| m.unread)
    });
    assert!(marked, "THREAD_ROW_UNREAD_MISSING");
}
