//! Each watch has its own handle: ending an old watch on a thread never ends a newer one on the same thread.

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[test]
fn ending_an_old_watch_keeps_a_newer_one() {
    let env = Env::new();
    env.append(2);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let old = client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 2), [1, 2]);
    // A reopened tab watches again (from the start) before the closed one's unwatch arrives.
    let new = client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 2), [1, 2]);
    client.unwatch(old);
    daemon.push_event(env.append(1));
    assert_eq!(seqs(&client, 1), [3], "the newer watch still delivers");

    client.unwatch(new);
    daemon.push_event(env.append(1));
    quiet(&client, Duration::from_millis(300));
}

#[test]
fn ending_an_old_watch_before_connecting_keeps_a_newer_one() {
    let env = Env::new();
    env.append(2);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    let old = client.watch(env.thread_id, 0);
    let _new = client.watch(env.thread_id, 0);
    client.unwatch(old);
    connected(&client);
    assert_eq!(seqs(&client, 2), [1, 2]);
    quiet(&client, Duration::from_millis(300));
}

#[test]
fn the_same_handle_twice_is_harmless() {
    let env = Env::new();
    env.append(1);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let a = client.watch(env.thread_id, 0);
    let b = client.watch(env.thread_id, 0);
    assert_ne!(a, b, "every watch gets its own handle");
    assert_eq!(seqs(&client, 1), [1]);
    client.unwatch(a);
    client.unwatch(a);
    daemon.push_event(env.append(1));
    assert_eq!(seqs(&client, 1), [2]);
}
