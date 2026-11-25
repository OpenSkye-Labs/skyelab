//! In-process calls run through the daemon's own backend, so they get its checks, not a shortcut around them.

mod common;

use skyelab_client::Error;
use skyelab_daemon::testing::Ran;
use skyelab_env::{LocalSource, Source};
use skyelab_protocol::ErrorCode;

#[test]
fn local_keeps_the_backend_checks() {
    let env = common::env();
    let local = LocalSource::new(env.store.clone(), env.backend());
    match local.shelve_thread(env.thread_id, true, Some(1)) {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::BadRequest),
        other => panic!("expected BadRequest, got {other:?}"),
    }
    assert_eq!(env.ran(), []);
    // An allowed call reaches the agents behind the backend, as a remote one does.
    local.interrupt(env.thread_id).expect("interrupt");
    assert_eq!(env.ran(), [Ran::Interrupt(env.thread_id)]);
}

#[test]
fn local_writes_reach_the_agents() {
    let env = common::env();
    let local = LocalSource::new(env.store.clone(), env.backend());
    let id = local.start_thread(env.project_id, "hello").expect("start");
    let events = local.thread_events_since(id, 0).expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(env.ran(), [Ran::Start(env.project_id, "hello".into())]);
}
