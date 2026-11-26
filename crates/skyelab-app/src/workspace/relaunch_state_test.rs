//! What a thread shows after a relaunch: its sidebar time is its last activity (not the status writes of the quit or
//! the start), and a turn Stop cut off still reads interrupted (row badge, "Stopped" turn end) until the next turn.

use super::thread::Stop;
use super::thread_model::{Item, Record};
use super::threads_test::resume::{reopen, send_text};
use super::threads_test::{
    Fixture, fixture, open_workspace, project_thread, setup, stored_records, type_and_send,
    wait_done, wait_until,
};
use super::*;
use crate::ui::Status;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_agent::Event;
use skyelab_store::Store;

/// The fixture's store as a file, so a relaunch can open it again.
fn on_disk(f: &mut Fixture) -> std::path::PathBuf {
    let path = f._home.path().join("store.db");
    f.store = Arc::new(Mutex::new(Store::open(&path).expect("store file")));
    path
}

/// The sidebar row of thread `id`: its status and time.
fn row(ws: &Entity<Workspace>, id: i64, cx: &mut VisualTestContext) -> (Option<Status>, Option<i64>) {
    ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .sections
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.id.as_ref() == format!("thread-{id}"))
            .map(|i| (i.status, i.updated_ms))
            .expect("the thread's row")
    })
}

fn stored_row(f: &Fixture, id: i64) -> skyelab_store::Thread {
    f.store.lock().expect("lock").thread(id).expect("read").expect("row")
}

/// When the thread's last event that is activity (a prompt or a result) was stored.
fn last_activity(f: &Fixture, id: i64) -> i64 {
    let store = f.store.lock().expect("lock");
    store
        .events_since(id, 0)
        .expect("events")
        .into_iter()
        .filter(|e| e.kind == "prompt" || e.payload_json.starts_with("{\"Result\""))
        .map(|e| e.created_at)
        .max()
        .expect("some activity")
}

fn relaunch(f: &mut Fixture, path: &std::path::Path) {
    std::thread::sleep(std::time::Duration::from_millis(20)); // "now" is later than any stored time
    f.store = Arc::new(Mutex::new(Store::open(path).expect("reopen")));
}

#[gpui::test]
fn a_turn_cut_off_by_stop_still_reads_interrupted_after_a_relaunch(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let path = on_disk(&mut f);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    // The fake waits on the answer, so the turn keeps running. A new thread's Full access would skip the ask.
    thread.update(cx, |t, _| t.mode = skyelab_agent::Mode::Default);
    send_text(&thread, "hello", cx);
    wait_done(&thread, cx); // the fake answers its launch prompt at once
    send_text(&thread, "permit", cx);
    wait_until(cx, "the turn waits on its ask", |cx| {
        thread.read_with(cx, |t, _| t.items.iter().any(|i| matches!(i, Item::Approval(_))))
    });
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "stopped and saved", |cx| {
        thread.read_with(cx, |t, _| t.session.is_none() && t.unsaved_events == 0)
    });
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    wait_until(cx, "the cut-off end stored", |_| {
        stored_records(&f.store, id).iter().any(|r| {
            matches!(r, Record::Agent(Event::Result { result: Some(e), .. }) if e == "interrupted")
        })
    });
    cx.run_until_parked();
    assert_eq!(row(&ws, id, cx).0, Some(Status::Interrupted), "live");
    let when = last_activity(&f, id);

    relaunch(&mut f, &path);
    let stored = stored_row(&f, id);
    assert_eq!(stored.status, "interrupted");
    assert_eq!(stored.updated_at, when, "the row's time is the turn's end, not the quit");
    let (ws, cx) = open_workspace(&f, cx);
    assert_eq!(row(&ws, id, cx), (Some(Status::Interrupted), Some(when)));
    let view = reopen(&f, &stored, cx);
    view.read_with(cx, |t, _| {
        assert!(
            matches!(t.items.last(), Some(Item::Done { is_error: true, error: Some(e), .. }) if e == "interrupted"),
            "the turn ends Stopped: {:?}",
            t.items.last()
        );
    });

    send_text(&view, "hello again", cx);
    wait_done(&view, cx);
    wait_until(cx, "the next turn clears it", |_| {
        let s = stored_row(&f, id).status;
        s != "interrupted" && s != "starting" && s != "running"
    });
    assert_ne!(view.read_with(cx, |t, _| t.live_status()), Status::Interrupted);
}

#[gpui::test]
fn a_finished_threads_time_survives_a_relaunch(cx: &mut TestAppContext) {
    setup(cx);
    let mut f = fixture(true);
    let path = on_disk(&mut f);
    let (ws, cx) = open_workspace(&f, cx);
    let thread = project_thread(&f, &ws, cx);
    type_and_send(&thread, cx);
    wait_done(&thread, cx);
    thread.update_in(cx, |t, window, cx| t.stop(&Stop, window, cx));
    wait_until(cx, "stopped", |cx| thread.read_with(cx, |t, _| t.session.is_none()));
    let id = thread.read_with(cx, |t, _| t.thread.as_ref().expect("row").id);
    wait_until(cx, "the stop written", |_| stored_row(&f, id).status == "stopped");
    let when = last_activity(&f, id);

    relaunch(&mut f, &path);
    let (ws, cx) = open_workspace(&f, cx);
    let (status, time) = row(&ws, id, cx);
    assert_eq!(time, Some(when), "the row's time is its turn's end, not the stop");
    assert!(status.is_some_and(|s| s.badge().is_none()), "a finished turn shows its time: {status:?}");
}
