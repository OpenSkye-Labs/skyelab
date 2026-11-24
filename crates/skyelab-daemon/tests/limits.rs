//! Per-connection limits: message size, requests in flight, idle time.

mod common;

use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use common::*;
use skyelab_daemon::testing::Peer;
use skyelab_daemon::{Backend, Config};
use skyelab_protocol::{
    Call, ErrorCode, Project, Reply, RpcError, ServerMessage, Thread, ThreadEvent,
};

#[test]
fn oversized_frame_closes_connection() {
    let config = Config {
        max_message_bytes: 1024,
        ..test_config()
    };
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        config,
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    let huge = format!(
        r#"{{"id":2,"method":"list_threads","params":{{"project_id":1}},"pad":"{}"}}"#,
        "x".repeat(4096)
    );
    // The send itself may fail if the server already hung up.
    let _ = ws.send(tungstenite::Message::text(huge));
    assert!(recv(&mut ws).is_none(), "server must close the connection");
}

#[test]
fn idle_connection_closed() {
    let config = Config {
        idle_timeout: Duration::from_millis(300),
        ..test_config()
    };
    let daemon = start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        config,
    );
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    let started = Instant::now();
    assert!(recv(&mut ws).is_none());
    assert!(
        started.elapsed() < Duration::from_secs(30), // the default idle timeout is 300 s
        "{:?}",
        started.elapsed()
    );
}

/// `list_projects` blocks until the test releases it.
struct Blocking {
    release: Mutex<mpsc::Receiver<()>>,
}

impl Backend for Blocking {
    fn list_projects(&self) -> Result<Vec<Project>, RpcError> {
        self.release
            .lock()
            .expect("test setup")
            .recv()
            .expect("test setup");
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
}

#[test]
fn in_flight_limit() {
    let (release, rx) = mpsc::channel();
    let backend = Arc::new(Blocking {
        release: Mutex::new(rx),
    });
    let config = Config {
        max_in_flight: 1,
        ..test_config()
    };
    let daemon = start(FakeTailnet::new(Peer::Own), backend, config);
    let mut ws = connect_hello(daemon.local_addrs()[0]);
    send(&mut ws, 2, Call::ListProjects);
    send(&mut ws, 3, Call::ListProjects);
    match recv(&mut ws) {
        Some(ServerMessage::Error { id: Some(3), error }) => {
            assert_eq!(error.code, ErrorCode::TooManyRequests)
        }
        other => panic!("{other:?}"),
    }
    release.send(()).expect("test setup");
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response {
            id: 2,
            result: Reply::Projects(_)
        })
    ));
    // The slot is free again.
    send(&mut ws, 4, Call::ListProjects);
    release.send(()).expect("test setup");
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response { id: 4, .. })
    ));
}
