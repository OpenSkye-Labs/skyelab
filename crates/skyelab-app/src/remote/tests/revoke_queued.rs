//! A call the daemon already read and queued for the Workspace doesn't run once its device is revoked.

use super::calls::{connect_hello, sharing_workspace};
use super::*;
use skyelab_protocol::device_key_hash;

#[gpui::test]
fn a_call_queued_before_a_revoke_never_runs(cx: &mut TestAppContext) {
    let (f, cx) = sharing_workspace(true, fake_bin(), cx);
    let client = connect_hello(&f, cx);
    let project_id = f.ws.read_with(cx, |ws, _| ws.project_ids()[0]);
    client.send(
        2,
        Call::StartThread {
            project_id,
            prompt: "hello from the phone".into(),
        },
    );
    // Not pumping the app, so the call waits in the Workspace's queue.
    let deadline = std::time::Instant::now() + crate::test_wait::DEADLINE;
    while f.queue.is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "the call never got queued"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    f.remote.update(cx, |r, cx| {
        r.revoke("nPhone", &device_key_hash(PHONE_KEY), cx)
    });
    wait_until(cx, "the queue to drain", |_| f.queue.is_empty());
    for _ in 0..50 {
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let threads = f
        .store
        .lock()
        .expect("store")
        .list_threads(project_id)
        .expect("threads");
    assert!(
        threads.is_empty(),
        "the revoked device's call ran: {threads:?}"
    );
}
