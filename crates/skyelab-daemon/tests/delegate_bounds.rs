//! `delegate` is bounded before the backend sees it: prompt and text 100 000 chars, title 200, one wait 20 s. A tunnel
//! connection can't call it, and each call takes one of the connection's `max_in_flight` permits.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use serde_json::Value;
use skyelab_daemon::testing::{Env, Peer};
use skyelab_daemon::{Backend, Config, Origin};
use skyelab_protocol::{
    Call, DelegateOp, Delegator, ErrorCode, Login, Project, RpcError, ServerMessage,
    TUNNEL_CAPABILITY, Thread, ThreadEvent, VERSION,
};

fn caller(title: &str) -> Delegator {
    Delegator {
        store_id: "s1".into(),
        thread_id: 7,
        title: title.into(),
        account_id: "x".into(),
        account_label: None,
        login: Login {
            provider: "claude".into(),
            email: "me@acme.com".into(),
            org_id: "org".into(),
        },
        depth: 0,
    }
}

fn code(ws: &mut Client, id: u64, caller: Delegator, op: DelegateOp) -> Option<ErrorCode> {
    send(ws, id, Call::Delegate { caller, op });
    match recv(ws) {
        Some(ServerMessage::Error { error, .. }) => Some(error.code),
        Some(ServerMessage::Response { .. }) => None,
        other => panic!("unexpected {other:?}"),
    }
}

fn start(prompt: String) -> DelegateOp {
    DelegateOp::Start {
        project_id: 1,
        prompt,
        worktree: false,
        request_id: "r1".into(),
    }
}

#[test]
fn oversized_calls_never_reach_the_backend() {
    let env = Env::start();
    let mut ws = connect_hello(env.daemon.as_ref().expect("served").local_addrs()[0]);
    let long = |n: usize| "é".repeat(n);
    let cases = [
        (caller("p"), start(long(100_001))),
        (caller(&long(201)), DelegateOp::ListProjects),
        (
            Delegator {
                account_label: Some(long(201)),
                ..caller("p")
            },
            DelegateOp::ListProjects,
        ),
        (
            caller("p"),
            DelegateOp::Send {
                thread_id: 1,
                text: long(100_001),
            },
        ),
        (
            caller("p"),
            DelegateOp::Wait {
                thread_id: 1,
                timeout_s: 21,
            },
        ),
        (caller("p"), start(String::new()).with_request_id("")),
        (
            caller("p"),
            start(String::new()).with_request_id(&"r".repeat(129)),
        ),
    ];
    for (i, (caller, op)) in cases.into_iter().enumerate() {
        assert_eq!(
            code(&mut ws, 10 + i as u64, caller, op),
            Some(ErrorCode::BadRequest),
            "case {i}"
        );
    }
    assert!(!env.asked().contains(&"delegate"), "{:?}", env.asked());
    // At the limits it reaches the backend (which refuses: no link).
    assert_eq!(
        code(
            &mut ws,
            30,
            Delegator {
                account_label: Some(long(200)),
                ..caller(&long(200))
            },
            start(long(100_000))
        ),
        Some(ErrorCode::DelegationNotAllowed)
    );
    assert!(env.asked().contains(&"delegate"));
}

trait WithRequest {
    fn with_request_id(self, id: &str) -> DelegateOp;
}

impl WithRequest for DelegateOp {
    fn with_request_id(self, id: &str) -> DelegateOp {
        match self {
            DelegateOp::Start {
                project_id,
                prompt,
                worktree,
                ..
            } => DelegateOp::Start {
                project_id,
                prompt,
                worktree,
                request_id: id.into(),
            },
            other => other,
        }
    }
}

#[test]
fn a_tunnel_connection_cannot_delegate() {
    let env = Env::start();
    let mut ws = connect(env.daemon.as_ref().expect("served").local_addrs()[0]);
    send(
        &mut ws,
        1,
        Call::Hello {
            protocol: VERSION,
            capabilities: vec![TUNNEL_CAPABILITY.into()],
            device_key: Some(KEY.into()),
        },
    );
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response { .. })
    ));
    assert_eq!(
        code(&mut ws, 2, caller("p"), DelegateOp::ListProjects),
        Some(ErrorCode::BadRequest)
    );
    assert!(!env.asked().contains(&"delegate"));
}

/// A backend whose `delegate` holds until released, counting the calls that got in.
#[derive(Default)]
struct Holding {
    entered: AtomicUsize,
    release: Mutex<bool>,
}

impl Backend for Holding {
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
        Ok(1)
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), RpcError> {
        Ok(())
    }
    fn interrupt(&self, _: i64) -> Result<(), RpcError> {
        Ok(())
    }
    fn delegate(&self, _: &Origin, _: Delegator, _: DelegateOp) -> Result<Value, RpcError> {
        self.entered.fetch_add(1, Ordering::SeqCst);
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !*self.release.lock().expect("release") && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(Value::Null)
    }
}

#[test]
fn each_delegate_call_takes_an_in_flight_permit() {
    let backend = Arc::new(Holding::default());
    let config = Config {
        max_in_flight: 2,
        ..test_config()
    };
    let daemon = common::start(FakeTailnet::new(Peer::Own), backend.clone(), config);
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    let wait = DelegateOp::Wait {
        thread_id: 1,
        timeout_s: 20,
    };
    for id in [2, 3] {
        send(
            &mut ws,
            id,
            Call::Delegate {
                caller: caller("p"),
                op: wait.clone(),
            },
        );
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while backend.entered.load(Ordering::SeqCst) < 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "both waits should be running"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        code(&mut ws, 4, caller("p"), wait),
        Some(ErrorCode::TooManyRequests)
    );
    *backend.release.lock().expect("release") = true;
    for _ in 0..2 {
        assert!(matches!(
            recv(&mut ws),
            Some(ServerMessage::Response { .. })
        ));
    }
}
