//! T3's Resume at reset: the stored schedule sends "Continue where you left off." once, in the thread's own session
//! and account, and a message the user sends first cancels it.

use super::resume::{env_reports, reopen, send_text, stopped_thread};
use super::*;
use skyelab_core::limit_reset::RESUME_PROMPT;

fn resume_prompts(f: &Fixture, id: i64) -> usize {
    stored_records(&f.store, id)
        .iter()
        .filter(|r| matches!(r, Record::Prompt(p) if p == RESUME_PROMPT))
        .count()
}

fn scheduled(f: &Fixture, id: i64) -> Option<i64> {
    f.store.lock().expect("lock").thread(id).expect("read").expect("row").limit_resume_at
}

#[gpui::test]
fn a_due_resume_continues_the_thread_once_under_its_account(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, ws, cx) = stopped_thread(&f, cx);
    // Scheduled before a restart, due now: the reloaded rows arm the timer, which sends it.
    let due = crate::ui::now_ms() - 1;
    f.store.lock().expect("lock").set_limit_resume(row.id, Some(due)).expect("schedule");
    ws.update(cx, |ws, cx| ws.load_threads(cx));
    wait_until(cx, "the resumed agent's env report", |_| {
        env_reports(&f.store, row.id).len() == 2
    });
    let resumed = &env_reports(&f.store, row.id)[1];
    let args = resumed["args"].as_str().expect("args");
    assert!(args.contains("--resume fake-sess-1"), "same session: {args}");
    assert_eq!(resumed["config_dir"], f.account_dir.to_str().expect("utf-8"), "its own account");
    wait_until(cx, "the resume prompt stored", |_| resume_prompts(&f, row.id) == 1);
    assert_eq!(scheduled(&f, row.id), None, "fired: the schedule is gone");

    // A second timer (another window, a late tick) finds nothing to send.
    f.store.lock().expect("lock").set_limit_resume(row.id, None).expect("clear");
    ws.update(cx, |ws, cx| ws.update_row(row.id, |t| t.limit_resume_at = Some(due), cx));
    ws.update_in(cx, |ws, window, cx| ws.send_due_resumes(window, cx));
    cx.run_until_parked();
    assert_eq!(resume_prompts(&f, row.id), 1, "sent once");
    assert_eq!(env_reports(&f.store, row.id).len(), 2, "no second agent");
}

#[gpui::test]
fn sending_first_cancels_the_resume(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, _ws, cx) = stopped_thread(&f, cx);
    let view = reopen(&f, &row, cx);
    let at = crate::ui::now_ms() + 3_600_000;
    view.update(cx, |v, cx| v.set_limit_resume(Some(at), cx));
    wait_until(cx, "the schedule stored", |_| scheduled(&f, row.id) == Some(at));
    send_text(&view, "again", cx);
    wait_until(cx, "the schedule cancelled", |_| scheduled(&f, row.id).is_none());
    view.read_with(cx, |v, _| {
        assert_eq!(v.thread.as_ref().and_then(|t| t.limit_resume_at), None);
    });
}
