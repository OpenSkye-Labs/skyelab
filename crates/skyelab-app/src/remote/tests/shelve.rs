//! Another device settles, snoozes and restores this machine's threads with `shelve_thread` (1.6): the store and the
//! sidebar's row change together, and the answer comes once both did. A thread not listed here is NotFound.

use super::calls::{connect_hello, error_of, sharing_workspace};
use super::*;
use crate::test_wait::wait_until;

#[gpui::test]
fn a_remote_client_settles_snoozes_and_restores_a_thread(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    let id = f
        .store
        .lock()
        .expect("store")
        .create_thread(&skyelab_store::NewThread {
            project_id,
            title: "alpha",
            cwd: Path::new("/src/demo"),
            account_id: "acme",
            status: "exited",
        })
        .expect("thread")
        .id;
    f.ws.update(cx, |ws, cx| ws.reload_threads(cx));
    wait_until(cx, "the row", |cx| {
        f.ws.read_with(cx, |ws, _| ws.row_shelf(id).is_some())
    });
    let client = connect_hello(&f, cx);
    let stored = || {
        let t = f.store.lock().expect("store").thread(id).expect("read");
        let t = t.expect("thread");
        (t.settled_at.is_some(), t.snoozed_until)
    };
    let until = crate::ui::now_ms() + 3_600_000;
    for (n, (settled, snoozed_until)) in [(true, None), (false, Some(until)), (false, None)]
        .into_iter()
        .enumerate()
    {
        let n = n as u64 + 2;
        client.send(
            n,
            Call::ShelveThread {
                thread_id: id,
                settled,
                snoozed_until,
            },
        );
        match client.reply(n, &mut Vec::new(), cx) {
            ServerMessage::Response {
                result: Answer::Ok, ..
            } => {}
            other => panic!("shelve failed: {other:?}"),
        }
        assert_eq!(stored(), (settled, snoozed_until), "the store");
        assert_eq!(
            f.ws.read_with(cx, |ws, _| ws.row_shelf(id)),
            Some((settled, snoozed_until)),
            "the sidebar's row"
        );
    }

    client.send(
        9,
        Call::ShelveThread {
            thread_id: id + 100,
            settled: true,
            snoozed_until: None,
        },
    );
    let error = error_of(client.reply(9, &mut Vec::new(), cx));
    assert_eq!(error.code, skyelab_protocol::ErrorCode::NotFound);
}
