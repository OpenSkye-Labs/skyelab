//! A remote follow-up to a stopped thread whose resume fails to start is answered with the failure.
#![cfg(unix)] // the fake agent is a shell script

use super::Backend;
use super::thread::ThreadView;
use super::threads_test::resume::stopped_thread;
use super::threads_test::{fixture, setup, wait_until};
use gpui::{TestAppContext, VisualContext as _};
use skyelab_core::AccountsConfig;

#[gpui::test]
fn remote_resume_that_fails_to_start_still_answers(cx: &mut TestAppContext) {
    setup(cx);
    let f = fixture(true);
    let (row, ws, cx) = stopped_thread(&f, cx);
    let config = AccountsConfig::load(&f.accounts_path).expect("load");
    // The agent binary is gone by the time the follow-up resumes the thread, so the relaunch fails.
    let backend = Backend {
        store: f.store.clone(),
        claude_bin: f.project_dir.join("no-such-claude"),
    };
    let reopened = row.clone();
    let view =
        cx.new_window_entity(|window, cx| ThreadView::open(reopened, config, backend, window, cx));
    wait_until(cx, "the row to load", |cx| {
        view.read_with(cx, |t, _| !t.read_only)
    });
    // What remote_send's resume arm does: submit, then answer once the agent runs or didn't start.
    view.update_in(cx, |v, window, cx| v.submit_remote("again", window, cx))
        .expect("taken: the thread resumes");
    let (tx, rx) = flume::bounded(1);
    ws.update(cx, |ws, cx| {
        ws.reply_when_started(&view, move |r| _ = tx.send(r.map(drop)), cx)
    });
    wait_until(cx, "the failed launch to end", |cx| {
        view.read_with(cx, |t, _| !t.starting && t.session.is_none())
    });
    let mut answer = None;
    wait_until(cx, "an answer", |_| {
        answer = rx.try_recv().ok();
        answer.is_some()
    });
    assert!(
        answer.expect("answered").is_err(),
        "a failed resume answers Err"
    );
}
