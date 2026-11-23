//! A working thread shows how long its turn has run: the sidebar row ("Working 1m 12s") and the thread view
//! ("Working for 1m 12s"), both from when the turn's prompt was sent.

use super::manage_harness::rig;
use super::threads_test::wait_until;
use super::*;
use crate::ui::elapsed;
use gpui::TestAppContext;

#[test]
fn elapsed_formats() {
    assert_eq!(elapsed(0), "0s");
    assert_eq!(elapsed(12_400), "12s");
    assert_eq!(elapsed(72_000), "1m 12s");
    assert_eq!(elapsed(65_000), "1m 05s");
    assert_eq!(elapsed(3_900_000), "1h 05m");
    assert_eq!(elapsed(-5), "0s", "a clock that stepped back reads as zero");
}

#[gpui::test]
fn working_row_has_elapsed(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    // The turn never ends, so it is still running when checked.
    std::fs::write(r.f.project_dir.join(".fake-agent-no-result"), "").expect("marker");
    let view = super::working_shelf_test::new_thread(&r, cx);
    let before = crate::ui::now_ms();
    view.update_in(cx, |v, window, cx| v.submit_remote("go", window, cx))
        .expect("sent");
    wait_until(cx, "the turn running", |cx| {
        view.read_with(cx, |v, _| v.turn_running())
    });
    let started = view.read_with(cx, |v, _| v.turn_started_ms);
    assert!(
        started.is_some_and(|s| s >= before && s <= crate::ui::now_ms()),
        "the view records when the turn started: {started:?}"
    );
    cx.run_until_parked();
    let id = view.read_with(cx, |v, _| v.thread.as_ref().expect("row").id);
    let sidebar = r.ws.read_with(cx, |ws, _| ws.sidebar.clone());
    let row = sidebar.read_with(cx, |s, _| {
        s.marks
            .get(&SharedString::from(format!("thread-{id}")))
            .and_then(|m| m.started_ms)
    });
    assert_eq!(row, started, "the row ticks from the same start");
}
