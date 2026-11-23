//! `answer_permission` (1.3) reaches the agents behind the daemon, and only after hello from an approved device.

mod common;

use std::time::Duration;

use common::*;
use skyelab_daemon::testing::{Env, NODE_NAME, Peer, Ran};
use skyelab_protocol::{Call, ErrorCode, PermissionAnswer};

#[test]
fn an_approved_client_answer_reaches_the_agent() {
    let env = Env::start();
    let client = skyelab_client::Client::connect(
        NODE_NAME,
        skyelab_client::testing::options(env.port, skyelab_daemon::testing::ca().root()),
    )
    .expect("client");
    match client.updates().recv_timeout(Duration::from_secs(5)) {
        Ok(skyelab_client::Update::Connected(_)) => {}
        other => panic!("expected Connected, got {other:?}"),
    }
    client
        .answer_permission(env.thread_id, "perm-1", PermissionAnswer::AllowThread)
        .recv()
        .expect("answered");
    assert_eq!(
        env.ran(),
        [Ran::Answer(
            env.thread_id,
            "perm-1".into(),
            PermissionAnswer::AllowThread
        )]
    );
}

#[test]
fn an_unapproved_device_cannot_answer() {
    let backend = std::sync::Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    let answer = Call::AnswerPermission {
        thread_id: 1,
        request_id: "perm-1".into(),
        decision: PermissionAnswer::AllowOnce,
    };
    send(&mut ws, 5, answer.clone());
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::HelloRequired);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(
        error_code(hello_key(&mut ws, Some(KEY))),
        ErrorCode::NotApproved
    );
    assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
