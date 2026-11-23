//! A call from a device that hung up is still cut off when that device is revoked afterwards.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_daemon::{Backend, CallGate};
use skyelab_protocol::{Call, Project, RpcError, Thread, ThreadEvent};

struct Slow {
    entered: flume::Sender<()>,
    release: flume::Receiver<()>,
    saw: flume::Sender<bool>,
}

impl Backend for Slow {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        Ok(vec![])
    }
    fn list_threads(&self, _: i64) -> Result<Vec<Thread>, RpcError> {
        Ok(vec![])
    }
    fn thread_events_since(&self, _: i64, _: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        Ok(vec![])
    }
    fn start_thread(&self, _: i64, _: &str) -> Result<i64, RpcError> {
        let gate = CallGate::current();
        self.entered.send(()).expect("test waits");
        self.release
            .recv_timeout(Duration::from_secs(10))
            .expect("released");
        self.saw.send(gate.cancelled()).expect("test waits");
        Ok(1)
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), RpcError> {
        Ok(())
    }
    fn interrupt(&self, _: i64) -> Result<(), RpcError> {
        Ok(())
    }
}

#[test]
fn a_revoke_after_the_device_hung_up_still_cancels_its_call() {
    let (entered, entered_rx) = flume::unbounded();
    let (release_tx, release) = flume::unbounded();
    let (saw, saw_rx) = flume::unbounded();
    let backend = Arc::new(Slow {
        entered,
        release,
        saw,
    });
    let daemon = start(FakeTailnet::new(Peer::Own), backend, test_config());
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(
        &mut ws,
        2,
        Call::StartThread {
            project_id: 1,
            prompt: "hi".into(),
        },
    );
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("the call reached the backend");

    // The device hangs up while its call is still running.
    let _ = ws.close(None);
    let _ = ws.flush();
    drop(ws);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !daemon.peers().is_empty() {
        assert!(Instant::now() < deadline, "the connection never closed");
        std::thread::sleep(Duration::from_millis(10));
    }

    daemon.set_approved(vec![]);
    release_tx.send(()).expect("backend waits");
    assert!(
        saw_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("answered"),
        "the revoked device's call was not cancelled"
    );
}
