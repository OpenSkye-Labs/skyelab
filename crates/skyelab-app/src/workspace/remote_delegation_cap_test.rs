//! Criterion 5: B holds a remote caller to 4 busy children per parent and 8 per origin machine, at every claim
//! site: a new start (two at once for the last slot), a follow-up that gives an idle child a new turn, a resume of
//! a stopped child, and the restart-resume of an interrupted turn.

use super::remote_delegation_harness::*;
use super::thread::ThreadView;
use super::*;
use crate::test_wait::wait_until;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_protocol::DelegateOp;

fn linked(disk: bool, cx: &mut TestAppContext) -> (B, &mut VisualTestContext) {
    b_side(
        Setup {
            link_to: Some("work"),
            disk_store: disk,
            ..Setup::default()
        },
        cx,
    )
}

fn status(b: &B, id: i64) -> String {
    b.store.lock().expect("lock").thread(id).expect("read").expect("row").status
}

fn caps_refuse_new_starts(b: &B, cx: &mut VisualTestContext) {
    seed_children(b, A_NODE, "sA", 7, 4, "running");
    let before = b.thread_count();
    let e = call(b, &caller(), start_op(b.project("work-a"), "fifth"), cx).expect_err("5th per parent");
    assert!(e.message.contains("running children"), "{}", e.message);
    assert_eq!(b.thread_count(), before);
}

#[gpui::test]
fn a_fifth_child_of_one_parent_is_refused(cx: &mut TestAppContext) {
    let (b, cx) = linked(false, cx);
    caps_refuse_new_starts(&b, cx);
}

#[gpui::test]
fn a_ninth_child_over_three_parents_is_refused(cx: &mut TestAppContext) {
    let (b, cx) = linked(false, cx);
    for (parent, n) in [(1, 3), (2, 3), (3, 2)] {
        seed_children(&b, A_NODE, "sA", parent, n, "running");
    }
    let before = b.thread_count();
    let e = call(&b, &caller(), start_op(b.project("work-a"), "ninth"), cx).expect_err("9th from that machine");
    assert!(e.message.contains("working here"), "{}", e.message);
    assert_eq!(b.thread_count(), before);
    // Another machine's callers aren't counted against it.
    seed_children(&b, "nOther", "sA", 7, 4, "running");
    let busy_from_a = b.store.lock().expect("lock").delegated_statuses(A_NODE, None).expect("read").len();
    assert_eq!(busy_from_a, 8);
}

#[gpui::test]
fn two_starts_at_once_for_the_last_slot_pass_once(cx: &mut TestAppContext) {
    let (b, cx) = linked(false, cx);
    seed_children(&b, A_NODE, "sA", 7, 3, "running");
    let first = b.client.delegate(caller(), start_op(b.project("work-a"), "one"));
    let second = b.client.delegate(caller(), start_op(b.project("work-a"), "two"));
    let (tx, rx) = flume::unbounded();
    for pending in [first, second] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(pending.recv_timeout(std::time::Duration::from_secs(60)));
        });
    }
    let mut answers = Vec::new();
    wait_until(cx, "both answers", |_| {
        answers.extend(rx.try_iter());
        answers.len() == 2
    });
    let passed = answers.iter().filter(|a| a.is_ok()).count();
    assert_eq!(passed, 1, "{answers:?}");
    let children = b.store.lock().expect("lock").delegated_statuses(A_NODE, Some(("sA", 7))).expect("read");
    assert_eq!(children.len(), 4, "one new row");
}

#[gpui::test]
fn a_send_to_an_idle_child_at_the_cap_is_refused(cx: &mut TestAppContext) {
    let (b, cx) = linked(false, cx);
    let child = started(&b, &caller(), start_op(b.project("work-a"), "r1"), cx);
    wait_until(cx, "the child's turn to end", |_| status(&b, child) == "idle");
    seed_children(&b, A_NODE, "sA", 7, 4, "running");
    let send = DelegateOp::Send {
        thread_id: child,
        text: "again".into(),
    };
    let e = call(&b, &caller(), send, cx).expect_err("at the cap");
    assert!(e.message.contains("running children"), "{}", e.message);
    assert_eq!(status(&b, child), "idle");
}

/// A stopped child of A's parent 7 with a session to resume, opened in a tab here.
fn stopped_child(b: &B, status: &str, cx: &mut VisualTestContext) -> (i64, Entity<ThreadView>) {
    let id = seed_children(b, A_NODE, "sA", 7, 1, status)[0];
    let thread = {
        let s = b.store.lock().expect("lock");
        s.set_agent_session_id(id, Some("sess-1")).expect("session");
        s.thread(id).expect("read").expect("row")
    };
    let (config, backend) = b.ws.read_with(cx, |ws, _| (ws.config.clone(), ws.backend.clone()));
    let view = b.ws.update_in(cx, |ws, window, cx| {
        let view = cx.new(|cx| ThreadView::open(thread, config, backend, window, cx));
        ws.add_thread_tab(view.clone(), false, window, cx);
        view
    });
    wait_until(cx, "the child to load as resumable", |cx| {
        view.read_with(cx, |v, _| !v.read_only && v.remote_parent.is_some())
    });
    (id, view)
}

#[gpui::test]
fn a_resume_at_the_cap_is_refused(cx: &mut TestAppContext) {
    let (b, cx) = linked(false, cx);
    let (id, view) = stopped_child(&b, "stopped", cx);
    seed_children(&b, A_NODE, "sA", 7, 4, "running");
    let _ = view.update_in(cx, |v, window, cx| v.submit_remote("keep going", window, cx));
    wait_until(cx, "the resume to settle", |cx| view.read_with(cx, |v, _| !v.starting));
    assert_eq!(status(&b, id), "stopped");
    let error = view.read_with(cx, |v, _| v.error.clone().map(|e| e.to_string())).unwrap_or_default();
    assert!(error.contains("running children"), "{error}");
}

#[gpui::test]
fn the_restart_resume_at_the_cap_is_refused(cx: &mut TestAppContext) {
    let (b, cx) = linked(true, cx);
    let id = seed_children(&b, A_NODE, "sA", 7, 1, "running")[0];
    {
        let mut s = b.store.lock().expect("lock");
        s.set_agent_session_id(id, Some("sess-1")).expect("session");
        s.append_event(id, "prompt", "\"work on it\"").expect("prompt");
    }
    // A restart: the store marks the running row interrupted and offers it for resume.
    let db = b.db.clone().expect("a store file");
    *b.store.lock().expect("lock") = Store::open(&db).expect("reopen");
    assert_eq!(status(&b, id), "interrupted");
    let siblings = seed_children(&b, A_NODE, "sA", 7, 4, "running");
    assert_eq!(siblings.len(), 4);
    b.ws.update_in(cx, |ws, window, cx| ws.resume_recovered_on_start(window, cx));
    let mut error = String::new();
    wait_until(cx, "the resume to be tried and refused", |cx| {
        let view = b.ws.read_with(cx, |ws, cx| ws.open_view(id, cx));
        error = view
            .and_then(|v| v.read_with(cx, |v, _| v.error.clone()))
            .map(|e| e.to_string())
            .unwrap_or_default();
        !error.is_empty()
    });
    assert!(error.contains("running children"), "{error}");
    assert!(
        !skyelab_core::delegation::is_busy(&status(&b, id)),
        "{}",
        status(&b, id)
    );
}
