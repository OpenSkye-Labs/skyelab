//! A call the daemon already read keeps running on its blocking thread after a revoke. The backend can see that
//! its connection was cut off ([`CallGate`]) and refuse before acting.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_daemon::{Backend, CallGate};
use skyelab_protocol::{Call, Project, RpcError, Thread, ThreadEvent};

/// `start_thread` waits for the test, then reports whether its call was cancelled by then.
struct Slow {
    entered: flume::Sender<()>,
    release: flume::Receiver<()>,
    saw: flume::Sender<bool>,
    gates: Mutex<Vec<CallGate>>,
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
        self.gates.lock().expect("gates").push(gate.clone());
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

fn slow() -> (
    Arc<Slow>,
    flume::Receiver<()>,
    flume::Sender<()>,
    flume::Receiver<bool>,
) {
    let (entered, entered_rx) = flume::unbounded();
    let (release_tx, release) = flume::unbounded();
    let (saw, saw_rx) = flume::unbounded();
    let backend = Arc::new(Slow {
        entered,
        release,
        saw,
        gates: Mutex::default(),
    });
    (backend, entered_rx, release_tx, saw_rx)
}

fn start_call(ws: &mut Client) {
    send(
        ws,
        2,
        Call::StartThread {
            project_id: 1,
            prompt: "hi".into(),
        },
    );
}

#[test]
fn a_revoke_cancels_the_call_its_backend_is_still_running() {
    let (backend, entered, release, saw) = slow();
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    start_call(&mut ws);
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("the call reached the backend");

    daemon.set_approved(vec![]);
    release.send(()).expect("backend waits");
    assert!(
        saw.recv_timeout(Duration::from_secs(10)).expect("answered"),
        "the call is cancelled"
    );
    assert!(recv(&mut ws).is_none(), "the revoked connection is closed");
}

#[test]
fn a_disconnect_cancels_it_too_and_other_calls_are_not_cancelled() {
    let (backend, entered, release, saw) = slow();
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    start_call(&mut ws);
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("reached");
    release.send(()).expect("backend waits");
    assert!(
        !saw.recv_timeout(Duration::from_secs(10)).expect("answered"),
        "nothing cut it off"
    );
    assert!(recv(&mut ws).is_some(), "answered");

    start_call(&mut ws);
    entered
        .recv_timeout(Duration::from_secs(10))
        .expect("reached");
    let peer = daemon.peers()[0].id;
    assert!(daemon.disconnect(peer));
    release.send(()).expect("backend waits");
    assert!(
        saw.recv_timeout(Duration::from_secs(10)).expect("answered"),
        "the call is cancelled"
    );

    assert!(
        !CallGate::current().cancelled(),
        "outside a call, nothing is cancelled"
    );
}
