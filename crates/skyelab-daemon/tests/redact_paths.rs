//! Errors a backend returns can carry this machine's paths (a folder named in a refusal). None reaches a client.

mod common;

use std::sync::Arc;

use common::*;
use skyelab_daemon::Backend;
use skyelab_daemon::testing::Peer;
use skyelab_protocol::{Call, ErrorCode, Project, RpcError, ServerMessage, Thread, ThreadEvent};

const MESSAGE: &str = "/Users/someone/My Projects/demo isn't bound to acme any more (C:\\Users\\someone\\demo, \
                       ~/demo, \"/tmp/x\"), so this thread can't continue.";

struct PathErrors;

fn err<T>() -> Result<T, RpcError> {
    Err(RpcError::new(ErrorCode::Internal, MESSAGE))
}

impl Backend for PathErrors {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        err()
    }
    fn list_threads(&self, _: i64) -> Result<Vec<Thread>, RpcError> {
        err()
    }
    fn thread_events_since(&self, _: i64, _: i64) -> Result<Vec<ThreadEvent>, RpcError> {
        err()
    }
    fn start_thread(&self, _: i64, _: &str) -> Result<i64, RpcError> {
        err()
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), RpcError> {
        err()
    }
    fn interrupt(&self, _: i64) -> Result<(), RpcError> {
        err()
    }
}

#[test]
fn backend_errors_reach_the_client_without_local_paths() {
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(PathErrors),
        test_config(),
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    let calls = [
        Call::ListProjects,
        Call::ListThreads { project_id: 1 },
        Call::ThreadEventsSince {
            thread_id: 1,
            seq: 0,
        },
        Call::StartThread {
            project_id: 1,
            prompt: "hi".into(),
        },
        Call::SendMessage {
            thread_id: 1,
            text: "hi".into(),
        },
        Call::Interrupt { thread_id: 1 },
    ];
    for (i, call) in calls.into_iter().enumerate() {
        let id = i as u64 + 2;
        send(&mut ws, id, call);
        let message = match recv(&mut ws) {
            Some(ServerMessage::Error { error, .. }) => error.message,
            other => panic!("expected an error, got {other:?}"),
        };
        for leak in ["/Users", "someone", "C:\\", "~/", "/tmp", "demo"] {
            assert!(!message.contains(leak), "{leak:?} leaked in {message:?}");
        }
        assert!(
            message.contains("isn't bound to acme any more")
                && message.contains("so this thread can't continue."),
            "the rest of the message stays: {message:?}"
        );
    }
}
