use serde_json::json;
use skyelab_protocol::{
    Call, ErrorCode, Hello, Project, Reply, Request, RpcError, ServerMessage, Thread, ThreadEvent,
    VERSION,
};

/// Serializes the request, checks the wire shape, and parses it back to the same value.
fn round_trip_request(call: Call, wire: serde_json::Value) {
    let req = Request { id: 7, call };
    let text = serde_json::to_string(&req).expect("test setup");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).expect("test setup"),
        wire
    );
    assert_eq!(Request::parse(&text).expect("test setup"), req);
}

fn round_trip_server(msg: ServerMessage) {
    let text = msg.to_json();
    assert_eq!(
        serde_json::from_str::<ServerMessage>(&text).expect("test setup"),
        msg,
        "{text}"
    );
}

fn event() -> ThreadEvent {
    ThreadEvent {
        thread_id: 3,
        seq: 2,
        kind: "assistant".into(),
        payload_json: r#"{"text":"hi"}"#.into(),
        created_at: 1_700_000_000_000,
    }
}

#[test]
fn round_trip_hello() {
    round_trip_request(
        Call::Hello {
            protocol: VERSION,
            capabilities: vec!["events".into()],
            device_key: Some("k".into()),
        },
        json!({"id": 7, "method": "hello", "params": {"protocol": {"major": 1, "minor": 2}, "capabilities": ["events"], "device_key": "k"}}),
    );
}

#[test]
fn hello_capabilities_are_optional() {
    let req =
        Request::parse(r#"{"id":1,"method":"hello","params":{"protocol":{"major":1,"minor":4}}}"#)
            .expect("test setup");
    assert!(matches!(req.call, Call::Hello { capabilities, .. } if capabilities.is_empty()));
}

#[test]
fn round_trip_list_projects() {
    round_trip_request(
        Call::ListProjects,
        json!({"id": 7, "method": "list_projects"}),
    );
}

#[test]
fn round_trip_list_threads() {
    round_trip_request(
        Call::ListThreads { project_id: 4 },
        json!({"id": 7, "method": "list_threads", "params": {"project_id": 4}}),
    );
}

#[test]
fn round_trip_thread_events_since() {
    round_trip_request(
        Call::ThreadEventsSince {
            thread_id: 3,
            seq: 10,
        },
        json!({"id": 7, "method": "thread_events_since", "params": {"thread_id": 3, "seq": 10}}),
    );
}

#[test]
fn round_trip_start_thread() {
    round_trip_request(
        Call::StartThread {
            project_id: 4,
            prompt: "fix it".into(),
        },
        json!({"id": 7, "method": "start_thread", "params": {"project_id": 4, "prompt": "fix it"}}),
    );
}

#[test]
fn round_trip_send_message() {
    round_trip_request(
        Call::SendMessage {
            thread_id: 3,
            text: "more".into(),
        },
        json!({"id": 7, "method": "send_message", "params": {"thread_id": 3, "text": "more"}}),
    );
}

#[test]
fn round_trip_interrupt() {
    round_trip_request(
        Call::Interrupt { thread_id: 3 },
        json!({"id": 7, "method": "interrupt", "params": {"thread_id": 3}}),
    );
}

#[test]
fn round_trip_every_reply() {
    let replies = [
        Reply::Hello(Hello {
            protocol: VERSION,
            capabilities: vec![],
            node_id: Some("nSelf".into()),
        }),
        Reply::Projects(vec![Project {
            id: 1,
            name: "app".into(),
            path: "/src/app".into(),
        }]),
        Reply::Threads(vec![Thread {
            id: 3,
            project_id: 1,
            title: "t".into(),
            account_id: "work".into(),
            account_label: Some("Work (Team)".into()),
            status: "idle".into(),
            created_at: 1,
            updated_at: 2,
        }]),
        Reply::Events(vec![event()]),
        Reply::ThreadStarted { thread_id: 3 },
        Reply::Ok,
    ];
    for result in replies {
        round_trip_server(ServerMessage::Response { id: 7, result });
    }
}

#[test]
fn round_trip_error() {
    round_trip_server(ServerMessage::Error {
        id: Some(7),
        error: RpcError::new(ErrorCode::NotWired, "later"),
    });
    round_trip_server(ServerMessage::Error {
        id: None,
        error: RpcError::new(ErrorCode::BadRequest, "garbage"),
    });
}

#[test]
fn round_trip_event_push() {
    let msg = ServerMessage::Event(event());
    assert_eq!(
        serde_json::to_value(&msg).expect("test setup")["type"],
        "event"
    );
    round_trip_server(msg);
}

#[test]
fn response_carries_request_id() {
    let v = serde_json::to_value(ServerMessage::Response {
        id: 42,
        result: Reply::Ok,
    })
    .expect("test setup");
    assert_eq!(
        v,
        json!({"type": "response", "id": 42, "result": {"kind": "ok"}})
    );
}

#[test]
fn unknown_method_is_bad_request_with_id() {
    let err = Request::parse(r#"{"id":5,"method":"rm_rf","params":{}}"#).unwrap_err();
    assert!(
        matches!(err, ServerMessage::Error { id: Some(5), error } if error.code == ErrorCode::BadRequest)
    );
}

#[test]
fn unreadable_request_is_bad_request_without_id() {
    let err = Request::parse("not json").unwrap_err();
    assert!(
        matches!(err, ServerMessage::Error { id: None, error } if error.code == ErrorCode::BadRequest)
    );
}

#[test]
fn unknown_error_code_from_newer_minor_parses() {
    let msg: ServerMessage = serde_json::from_str(
        r#"{"type":"error","id":1,"error":{"code":"quota_exceeded","message":"x"}}"#,
    )
    .expect("test setup");
    assert!(matches!(msg, ServerMessage::Error { error, .. } if error.code == ErrorCode::Unknown));
}

#[test]
fn device_keys_hash_and_code() {
    let key = skyelab_protocol::new_device_key().expect("rng");
    assert!(skyelab_protocol::is_device_key(&key));
    assert_ne!(skyelab_protocol::new_device_key().expect("rng"), key);
    let hash = skyelab_protocol::device_key_hash(&key);
    assert_eq!(hash.len(), 64);
    assert_eq!(hash, skyelab_protocol::device_key_hash(&key));
    let code = skyelab_protocol::device_code(&hash);
    assert_eq!(
        code,
        format!("{}-{}", &hash[..4], &hash[4..8]).to_uppercase()
    );
    // SHA-256("abc"), so the hash is the standard one, not something only this code agrees with.
    assert!(skyelab_protocol::device_key_hash("abc").starts_with("ba7816bf"));
    assert!(!skyelab_protocol::is_device_key(&key.to_uppercase()));
}
