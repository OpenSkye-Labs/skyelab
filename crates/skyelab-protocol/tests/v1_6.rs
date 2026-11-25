//! Protocol 1.6: `shelve_thread` and a thread's settled and snoozed state. Both are optional on the wire, so a 1.5
//! peer's threads still parse, and a 1.5 server reading the call fails it as an unknown method, not a crash.

use serde_json::json;
use skyelab_protocol::{Call, ErrorCode, Request, ServerMessage, Thread};

#[test]
fn shelve_thread_round_trips() {
    for (settled, snoozed_until, wire) in [
        (
            true,
            None,
            json!({"id": 9, "method": "shelve_thread", "params": {"thread_id": 3, "settled": true}}),
        ),
        (
            false,
            Some(5),
            json!({"id": 9, "method": "shelve_thread",
                   "params": {"thread_id": 3, "settled": false, "snoozed_until": 5}}),
        ),
    ] {
        let req = Request {
            id: 9,
            call: Call::ShelveThread {
                thread_id: 3,
                settled,
                snoozed_until,
            },
        };
        assert_eq!(serde_json::to_value(&req).expect("serializes"), wire);
        assert_eq!(Request::parse(&wire.to_string()).expect("parses"), req);
    }
}

#[test]
fn a_1_5_thread_has_no_shelf() {
    let old = json!({"id": 3, "project_id": 1, "title": "t", "account_id": "work", "status": "idle",
                     "created_at": 1, "updated_at": 2});
    let thread: Thread = serde_json::from_value(old).expect("a 1.5 thread parses");
    assert_eq!((thread.settled_at, thread.snoozed_until), (None, None));
    let shelved = Thread {
        settled_at: Some(7),
        ..thread
    };
    let wire = serde_json::to_value(&shelved).expect("serializes");
    assert_eq!(wire["settled_at"], 7);
    assert!(wire.get("snoozed_until").is_none());
}

/// What an older server answers: its `Call` has no such variant, so parsing fails as a bad request with the id.
#[test]
fn an_unknown_shelve_call_is_a_bad_request_with_its_id() {
    let text = r#"{"id": 4, "method": "shelve_later", "params": {"thread_id": 3}}"#;
    match Request::parse(text) {
        Err(ServerMessage::Error { id, error }) => {
            assert_eq!(id, Some(4));
            assert_eq!(error.code, ErrorCode::BadRequest);
        }
        other => panic!("expected a bad request, got {other:?}"),
    }
}
