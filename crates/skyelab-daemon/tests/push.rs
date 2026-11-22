//! Server-pushed thread events.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::*;
use skyelab_protocol::{Reply, ServerMessage, ThreadEvent, VERSION};

fn event(seq: i64) -> ThreadEvent {
    ThreadEvent {
        thread_id: 3,
        seq,
        kind: "assistant".into(),
        payload_json: "{}".into(),
        created_at: 1,
    }
}

#[test]
fn pushes_events_after_hello() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        test_config(),
    );
    let addr = daemon.local_addrs()[0];
    let mut ready = connect_hello(addr);
    let mut silent = connect(addr);
    daemon.push_event(event(1));
    assert_eq!(recv(&mut ready), Some(ServerMessage::Event(event(1))));
    // A connection that hadn't said hello got nothing: its first message is the hello reply.
    match hello_with(&mut silent, VERSION) {
        Some(ServerMessage::Response {
            id: 1,
            result: Reply::Hello(_),
        }) => {}
        other => panic!("{other:?}"),
    }
    daemon.push_event(event(2));
    assert_eq!(recv(&mut silent), Some(ServerMessage::Event(event(2))));
    assert_eq!(recv(&mut ready), Some(ServerMessage::Event(event(2))));
}
