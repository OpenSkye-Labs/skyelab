//! `answer_questions` (1.4) reaches the agents behind the daemon, and only after hello from an approved device;
//! a revoked device's connection is closed, so it can't answer either.

mod common;

use std::time::Duration;

use common::*;
use skyelab_daemon::testing::{Env, NODE_NAME, Peer, Ran};
use skyelab_protocol::{Call, ErrorCode};

fn answer() -> Call {
    Call::AnswerQuestions {
        thread_id: 1,
        request_id: "ask-1".into(),
        answers: vec![vec!["Rust".into()]],
    }
}

#[test]
fn an_approved_client_answers_questions() {
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
    let answers = vec![vec!["Rust".to_owned()], vec!["Tests".into(), "Docs".into()]];
    client
        .answer_questions(env.thread_id, "ask-1", answers.clone())
        .recv()
        .expect("answered");
    assert_eq!(
        env.ran(),
        [Ran::Questions(env.thread_id, "ask-1".into(), answers)]
    );
}

#[test]
fn an_unapproved_device_cannot_answer_questions() {
    let backend = std::sync::Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    daemon.set_approved(vec![]);
    let mut ws = connect(daemon.local_addrs()[0]);
    send(&mut ws, 5, answer());
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::HelloRequired);
    let mut ws = connect(daemon.local_addrs()[0]);
    assert_eq!(
        error_code(hello_key(&mut ws, Some(KEY))),
        ErrorCode::NotApproved
    );
    assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_revoked_device_cannot_answer_questions() {
    let backend = std::sync::Arc::new(Counting::default());
    let daemon = start(FakeTailnet::new(Peer::Own), backend.clone(), test_config());
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    daemon.set_approved(vec![]);
    send(&mut ws, 5, answer());
    assert!(
        !matches!(
            recv(&mut ws),
            Some(skyelab_protocol::ServerMessage::Response { .. })
        ),
        "a revoked connection gets no answer"
    );
    assert_eq!(backend.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
