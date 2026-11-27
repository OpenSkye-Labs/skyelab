//! Protocol 1.8: `delegate`, an agent on another of the user's machines starting and driving threads here. Every
//! addition is optional or a new variant, so 1.7 peers' messages still parse.

use serde::Deserialize;
use serde_json::json;
use skyelab_protocol::{
    Call, DELEGATION_SINCE_MINOR, DelegateOp, DelegatedFrom, Delegator, ErrorCode, Login, Reply,
    Request, RpcError, ServerMessage, Thread, ThreadRow, VERSION, Version, negotiate,
};

fn caller() -> Delegator {
    Delegator {
        store_id: "s1".into(),
        thread_id: 7,
        title: "parent".into(),
        account_id: "work".into(),
        account_label: None,
        login: Login {
            provider: "claude".into(),
            email: "me@acme.com".into(),
            org_id: "org".into(),
        },
        depth: 1,
    }
}

#[test]
fn every_delegate_op_round_trips() {
    assert_eq!(VERSION, Version { major: 1, minor: 8 });
    assert_eq!(DELEGATION_SINCE_MINOR, 8);
    let ops = [
        DelegateOp::ListProjects,
        DelegateOp::Start {
            project_id: 2,
            prompt: "do it".into(),
            worktree: true,
            request_id: "r1".into(),
        },
        DelegateOp::Status { thread_id: 3 },
        DelegateOp::Read {
            thread_id: 3,
            since_seq: 4,
        },
        DelegateOp::Send {
            thread_id: 3,
            text: "more".into(),
        },
        DelegateOp::Wait {
            thread_id: 3,
            timeout_s: 20,
        },
        DelegateOp::Interrupt { thread_id: 3 },
    ];
    for op in ops {
        let req = Request {
            id: 9,
            call: Call::Delegate {
                caller: caller(),
                op,
            },
        };
        let text = serde_json::to_string(&req).expect("serializes");
        assert_eq!(Request::parse(&text).expect("parses"), req, "{text}");
    }
    let reply = ServerMessage::Response {
        id: 9,
        result: Reply::Delegated(json!({"thread_id": 5})),
    };
    let back: ServerMessage = serde_json::from_str(&reply.to_json()).expect("parses");
    assert_eq!(back, reply);
}

#[test]
fn an_unknown_op_is_a_bad_request() {
    let text = json!({"id": 1, "method": "delegate", "params": {"caller": caller(), "op": {"op": "format_disk"}}})
        .to_string();
    match Request::parse(&text) {
        Err(ServerMessage::Error { id, error }) => {
            assert_eq!(id, Some(1));
            assert_eq!(error.code, ErrorCode::BadRequest);
        }
        other => panic!("expected bad_request, got {other:?}"),
    }
}

/// The 1.7 error codes, as a 1.7 client reads them.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Code17 {
    UnsupportedVersion,
    HelloRequired,
    BadRequest,
    NotFound,
    TooManyRequests,
    NotWired,
    Internal,
    NotApproved,
    Denied,
    #[serde(other)]
    Unknown,
}

#[test]
fn the_new_error_code_is_additive() {
    let wire = serde_json::to_value(RpcError::new(ErrorCode::DelegationNotAllowed, "no link"))
        .expect("serializes");
    assert_eq!(wire["code"], "delegation_not_allowed");
    let old: Code17 = serde_json::from_value(wire["code"].clone()).expect("a 1.7 client parses it");
    assert_eq!(old, Code17::Unknown);
}

#[test]
fn messages_from_1_7_still_parse_and_1_8_hellos_negotiate() {
    let hello =
        json!({"id": 1, "method": "hello", "params": {"protocol": {"major": 1, "minor": 7}}});
    assert!(Request::parse(&hello.to_string()).is_ok());
    for minor in [0, 7, 8, 99] {
        assert_eq!(
            negotiate(Version { major: 1, minor }).expect("1.x"),
            VERSION
        );
    }
    let old = json!({"id": 3, "project_id": 1, "title": "t", "account_id": "work", "status": "idle",
                     "created_at": 1, "updated_at": 2, "cwd": "/a", "parent_thread_id": 2});
    let t: Thread = serde_json::from_value(old).expect("a 1.7 thread parses");
    assert_eq!(t.row.delegated_from, None);
}

#[test]
fn delegated_from_rides_on_the_thread_row() {
    let row = ThreadRow {
        delegated_from: Some(DelegatedFrom {
            machine: "a-mac.example.ts.net".into(),
            parent_thread_id: 7,
            parent_title: "parent".into(),
        }),
        ..ThreadRow::default()
    };
    let t = Thread {
        id: 1,
        row,
        ..Thread::default()
    };
    let wire = serde_json::to_value(&t).expect("serializes");
    assert_eq!(wire["delegated_from"]["machine"], "a-mac.example.ts.net");
    assert_eq!(serde_json::from_value::<Thread>(wire).expect("parses"), t);
    let empty = serde_json::to_value(Thread::default()).expect("serializes");
    assert!(empty.get("delegated_from").is_none());
}

#[test]
fn the_account_label_is_optional_on_the_wire() {
    let without = serde_json::to_value(caller()).expect("serializes");
    assert!(without.get("account_label").is_none(), "{without}");
    // What an older 1.8 peer sends parses, with no label.
    let parsed: Delegator = serde_json::from_value(without).expect("parses");
    assert_eq!(parsed.account_label, None);
    let labelled = Delegator {
        account_label: Some("Work".into()),
        ..caller()
    };
    let wire = serde_json::to_value(&labelled).expect("serializes");
    assert_eq!(wire["account_label"], "Work");
    assert_eq!(
        serde_json::from_value::<Delegator>(wire).expect("parses"),
        labelled
    );
}
