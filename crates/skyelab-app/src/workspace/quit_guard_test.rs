//! Acceptance: Quit with nothing in flight quits at once; with a thread waiting on the user it asks (the dialog lists it),
//! Cancel keeps the app, a second Quit while the dialog is open quits, and "Don't ask again" quits at once.

use super::manage_harness::rig;
use super::quit_guard::{Blocker, TestQuits, asks, blockers, question, request_quit, set_asks};
use super::threads_test::wait_until;
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use gpui_component::WindowExt as _;

fn quits(cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| cx.try_global::<TestQuits>().map_or(0, |q| q.0))
}

/// Quit from the app level, as the menu sends it (a test's `cx.update` runs inside the window's own update).
fn quit(cx: &mut VisualTestContext) {
    TestAppContext::update(cx, request_quit);
    cx.run_until_parked();
}

fn dialog_open(cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| window.has_active_dialog(cx))
}

#[test]
fn the_question_counts_each_kind() {
    let items = |kinds: &[Blocker]| -> Vec<(SharedString, Blocker)> {
        kinds.iter().map(|b| ("x".into(), *b)).collect()
    };
    let w = Blocker::Working;
    let (title, body) = question(&items(&[w, w, w]));
    assert_eq!(title.as_ref(), "Quit SkyeLab?");
    assert_eq!(body.as_ref(), "3 threads are working. Quitting stops them.");
    let (_, body) = question(&items(&[
        w,
        Blocker::Waiting(panels::Ask::Approval),
        Blocker::Terminal,
        Blocker::Terminal,
    ]));
    let (_, one) = question(&items(&[w]));
    assert_eq!(one.as_ref(), "1 thread is working. Quitting stops it.");
    assert_eq!(
        body.as_ref(),
        "1 thread is working, 1 thread is waiting for you and 2 terminals are running commands. Quitting stops them."
    );
}

#[gpui::test]
fn quit_asks_only_while_a_thread_waits(cx: &mut TestAppContext) {
    let (r, cx) = rig(cx);
    cx.update(|_, cx| cx.set_global(TestQuits(0)));
    quit(cx);
    assert_eq!(quits(cx), 1, "nothing in flight: quits at once");
    assert!(!dialog_open(cx));

    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = r.ws.read_with(cx, |ws, _| ws.open_threads.last().cloned()).expect("a thread tab");
    // The agent asks to run a command and waits for the answer: a steady "waiting" state.
    view.update_in(cx, |v, window, cx| {
        v.mode = skyelab_agent::Mode::Default; // a new thread's Full access would skip the ask
        v.submit_remote("permit", window, cx)
    })
        .expect("sent");
    wait_until(cx, "the agent to wait on an approval", |cx| {
        cx.update(|_, cx| blockers(cx).iter().any(|(_, b)| matches!(b, Blocker::Waiting(_))))
    });
    let title = view.read_with(cx, |v, _| v.title());
    assert_eq!(
        cx.update(|_, cx| blockers(cx)),
        vec![(title, Blocker::Waiting(panels::Ask::Approval))]
    );

    quit(cx);
    assert!(dialog_open(cx), "a waiting thread: the dialog asks");
    assert_eq!(quits(cx), 1, "and the app keeps running");

    cx.update(|window, cx| window.close_dialog(cx)); // the dialog closes, as Cancel or Escape close it
    cx.run_until_parked();
    quit(cx);
    assert!(dialog_open(cx), "after Cancel, Quit asks again");
    assert_eq!(quits(cx), 1);

    quit(cx);
    assert_eq!(quits(cx), 2, "a second Quit while the dialog is open quits");

    cx.update(|window, cx| window.close_dialog(cx));
    cx.run_until_parked();
    cx.update(|_, cx| set_asks(false, cx)).expect("saved");
    assert!(!cx.update(|_, cx| asks(cx)));
    quit(cx);
    assert_eq!(quits(cx), 3, "Don't ask again: quits at once");
    assert!(!dialog_open(cx));
}

#[test]
fn unsent_work_alone_is_lost_not_stopped() {
    let one = [("x".into(), Blocker::Unsent)];
    let (_, body) = question(&one);
    assert_eq!(
        body.as_ref(),
        "1 thread has unsent comments or context. Quitting loses it."
    );
    let (_, body) = question(&[("x".into(), Blocker::Working), ("y".into(), Blocker::Unsent)]);
    assert_eq!(
        body.as_ref(),
        "1 thread is working and 1 thread has unsent comments or context. Quitting stops them."
    );
}

#[gpui::test]
fn unsent_comments_and_chips_ask_before_quitting(cx: &mut TestAppContext) {
    use crate::workspace::changes_model::{ChangedFile, Changes, LineKind, Row, Scope};
    use crate::workspace::thread::composer::composer_context::ChipKind;
    let (r, cx) = rig(cx);
    cx.dispatch_action(NewThread);
    cx.run_until_parked();
    let view = r.ws.read_with(cx, |ws, _| ws.open_threads.last().cloned()).expect("a thread tab");
    assert!(cx.update(|_, cx| blockers(cx)).is_empty(), "an idle, empty thread");
    let title = view.read_with(cx, |v, _| v.title());

    // A diff comment saved on the Changes tab and not yet sent: memory only.
    let changes = Changes {
        files: vec![ChangedFile {
            path: "a.rs".into(),
            old_path: None,
            kind: skyelab_git::ChangeKind::Modified,
            binary: false,
            first_row: 0,
        }],
        rows: vec![
            Row::File(0),
            Row::Line { old: None, new: Some(1), kind: LineKind::Added, text: "a".into() },
        ],
        ..Default::default()
    };
    view.update(cx, |v, _| v.composer.review.add(&changes, Scope::Working, 1, 1, "why?"))
        .expect("added");
    assert_eq!(cx.update(|_, cx| blockers(cx)), vec![(title.clone(), Blocker::Unsent)]);
    quit(cx);
    assert!(dialog_open(cx), "unsent comments: the dialog asks");
    cx.update(|window, cx| window.close_dialog(cx));
    cx.run_until_parked();

    // Sent into the composer as a chip, still unsent.
    view.update(cx, |v, cx| {
        v.composer.review.clear(Scope::Working);
        assert!(v.push_context(ChipKind::Review, "1 comment".into(), "why?".into(), None, cx));
    });
    assert_eq!(cx.update(|_, cx| blockers(cx)), vec![(title, Blocker::Unsent)]);
}
