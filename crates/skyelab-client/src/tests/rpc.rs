//! Hello, then each method against the real daemon and store.

use std::sync::Arc;

use super::*;
use crate::{Error, ErrorCode};
use skyelab_protocol::VERSION;

#[test]
fn hello_then_every_method() {
    let env = Env::new();
    // Every agent call fails like a desktop with no agents wired up.
    env.agents
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    append(&env, 3);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    match next(&client) {
        Update::Connected(h) => {
            assert_eq!(h.protocol.major, VERSION.major);
            assert_eq!(client.hello(), Some(h));
        }
        other => panic!("{other:?}"),
    }

    let projects = client.list_projects().recv().expect("projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(
        (projects[0].id, projects[0].name.as_str()),
        (env.project_id, "app")
    );

    let threads = client.list_threads(env.project_id).recv().expect("threads");
    assert_eq!(threads.len(), 1);
    assert_eq!(
        (threads[0].id, threads[0].title.as_str()),
        (env.thread_id, "fix it")
    );

    let events = client
        .thread_events_since(env.thread_id, 1)
        .recv()
        .expect("events");
    assert_eq!(events.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3]);

    // Each write comes back as the daemon's NotWired error.
    let not_wired = |r: Result<_, Error>| match r {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::NotWired),
        other => panic!("expected NotWired, got {other:?}"),
    };
    not_wired(client.start_thread(env.project_id, "hi").recv().map(|_| ()));
    not_wired(client.send_message(env.thread_id, "more").recv());
    not_wired(client.interrupt(env.thread_id).recv());
}

/// A backend whose writes succeed, to check the Ok replies map to values.
struct Writes;

impl skyelab_daemon::Backend for Writes {
    fn list_projects(&self) -> Result<Vec<skyelab_protocol::Project>, skyelab_protocol::RpcError> {
        Ok(vec![])
    }
    fn list_threads(
        &self,
        _: i64,
    ) -> Result<Vec<skyelab_protocol::Thread>, skyelab_protocol::RpcError> {
        Ok(vec![])
    }
    fn thread_events_since(
        &self,
        _: i64,
        _: i64,
    ) -> Result<Vec<skyelab_protocol::ThreadEvent>, skyelab_protocol::RpcError> {
        Ok(vec![])
    }
    fn start_thread(&self, project_id: i64, _: &str) -> Result<i64, skyelab_protocol::RpcError> {
        Ok(project_id * 10)
    }
    fn send_message(&self, _: i64, _: &str) -> Result<(), skyelab_protocol::RpcError> {
        Ok(())
    }
    fn interrupt(&self, _: i64) -> Result<(), skyelab_protocol::RpcError> {
        Ok(())
    }
}

#[test]
fn writes_return_their_values() {
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), Arc::new(Writes), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    assert_eq!(client.start_thread(4, "go").recv(), Ok(40));
    assert_eq!(client.send_message(40, "more").recv(), Ok(()));
    assert_eq!(client.interrupt(40).recv(), Ok(()));
}

#[test]
fn request_while_disconnected_fails_fast() {
    // Nothing listens on this port.
    let free = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = free.local_addr().expect("addr").port();
    drop(free);
    let client = Client::connect(HOST, options(port)).expect("client");
    assert_eq!(client.list_projects().recv(), Err(Error::NotConnected));
}
