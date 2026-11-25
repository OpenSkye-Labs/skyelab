//! Settle and snooze on another machine's thread (protocol 1.6): the row's buttons ask that machine, whose store
//! changes, and the row moves to the Settled or Snoozed shelf here. While the machine is offline the action is
//! refused with a note and nothing is sent.

use super::env_harness::*;
use super::thread_list::{Shelf, ShelveThread};
use super::*;
use gpui::{TestAppContext, VisualTestContext};
use skyelab_daemon::testing::NODE_NAME;

const TITLE: &str = "fix the login bug";

fn shelve(ws: &Entity<Workspace>, id: i64, to: Shelf, cx: &mut VisualTestContext) {
    let a = ShelveThread {
        id,
        to,
        host: Some(NODE_NAME.into()),
    };
    ws.update_in(cx, |ws, window, cx| ws.shelve_thread(&a, window, cx));
}

/// The sidebar's shelves as (title, row labels).
fn shelves(ws: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<(String, Vec<String>)> {
    ws.read_with(cx, |ws, cx| {
        ws.sidebar
            .read(cx)
            .shelves
            .iter()
            .map(|s| {
                let rows = s.items.iter().map(|i| i.label.to_string()).collect();
                (s.title.to_string(), rows)
            })
            .collect()
    })
}

fn stored(remote: &RemoteSide) -> (bool, Option<i64>) {
    let store = remote.store.lock().expect("store");
    let t = store.thread(remote.thread_id).expect("read").expect("thread");
    (t.settled_at.is_some(), t.snoozed_until)
}

#[gpui::test]
fn a_remote_thread_is_settled_snoozed_and_back_on_its_machine(cx: &mut TestAppContext) {
    let remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    wait_until(cx, "the remote thread", |cx| sidebar(&ws, cx)[1].1 == [TITLE]);
    let row = SharedString::from(format!("remote-thread-{}", remote.thread_id));
    let host = ws.read_with(cx, |ws, cx| {
        let marks = &ws.sidebar.read(cx).marks;
        marks[&row].shelf.as_ref().and_then(|s| s.host.clone())
    });
    assert_eq!(host.as_deref(), Some(NODE_NAME), "the row offers its machine's shelf");

    shelve(&ws, remote.thread_id, Shelf::Settle, cx);
    wait_until(cx, "the Settled shelf", |cx| {
        shelves(&ws, cx) == [("Settled".to_owned(), vec![TITLE.to_owned()])]
    });
    assert_eq!(stored(&remote), (true, None), "settled on its machine");
    assert!(sidebar(&ws, cx)[1].1.is_empty(), "out of the list");
    let notices = ws.read_with(cx, |ws, _| ws.notices.clone());
    assert!(notices.iter().any(|n| n == &format!("Settled “{TITLE}”")), "{notices:?}");

    let until = crate::ui::now_ms() + 3_600_000;
    shelve(&ws, remote.thread_id, Shelf::Snooze(until), cx);
    wait_until(cx, "the Snoozed shelf", |cx| {
        shelves(&ws, cx) == [("Snoozed".to_owned(), vec![TITLE.to_owned()])]
    });
    assert_eq!(stored(&remote), (false, Some(until)));

    shelve(&ws, remote.thread_id, Shelf::Wake, cx);
    wait_until(cx, "back in the list", |cx| sidebar(&ws, cx)[1].1 == [TITLE]);
    assert_eq!(stored(&remote), (false, None));
    assert!(shelves(&ws, cx).is_empty());
    assert_eq!(
        remote.ran(),
        [
            Ran::Shelve(remote.thread_id, true, None),
            Ran::Shelve(remote.thread_id, false, Some(until)),
            Ran::Shelve(remote.thread_id, false, None),
        ]
    );
}

#[gpui::test]
fn an_offline_machine_is_not_asked(cx: &mut TestAppContext) {
    let mut remote = remote_side(FakeTailnet::default());
    let (ws, _local, cx) = local_workspace(Arc::default(), None, remote.options(), cx);
    connect(&ws, &remote, cx);
    wait_until(cx, "the remote thread", |cx| sidebar(&ws, cx)[1].1 == [TITLE]);
    remote.stop();
    wait_until(cx, "offline", |cx| state(&ws, cx) != "Connected");

    shelve(&ws, remote.thread_id, Shelf::Settle, cx);
    let notices = ws.read_with(cx, |ws, _| ws.notices.clone());
    assert!(
        notices.iter().any(|n| n.contains("is offline")),
        "{notices:?}"
    );
    assert_eq!(stored(&remote), (false, None));
    assert!(remote.ran().is_empty());
}
