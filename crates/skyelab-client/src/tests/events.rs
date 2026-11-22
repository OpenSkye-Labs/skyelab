//! Watched threads get every event once, in order, past the 500-event page cap and across gaps.

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[test]
fn watch_pages_past_the_reply_cap() {
    let env = Env::new();
    append(&env, 1200);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let _watch = client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 1200), (1..=1200).collect::<Vec<_>>());
    quiet(&client, Duration::from_millis(300));
}

#[test]
fn pushed_gap_is_filled_and_duplicates_dropped() {
    let env = Env::new();
    append(&env, 2);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let _watch = client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 2), [1, 2]);

    // In order: delivered straight from the push.
    daemon.push_event(append(&env, 1));
    assert_eq!(seqs(&client, 1), [3]);

    // Seqs 4 and 5 were never pushed: the push of 6 is a gap, filled from the store.
    daemon.push_event(append(&env, 3));
    assert_eq!(seqs(&client, 3), [4, 5, 6]);

    // A duplicate push of an old seq, and a push for an unwatched thread, deliver nothing.
    let mut dup = append(&env, 1);
    let seven = dup.clone();
    dup.seq = 3;
    daemon.push_event(dup);
    let mut other = seven.clone();
    other.thread_id = env.thread_id + 100;
    daemon.push_event(other);
    daemon.push_event(seven);
    assert_eq!(seqs(&client, 1), [7]);
    quiet(&client, Duration::from_millis(300));
}

#[test]
fn watch_from_a_seq_and_unwatch() {
    let env = Env::new();
    append(&env, 5);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    let watch = client.watch(env.thread_id, 3);
    assert_eq!(seqs(&client, 2), [4, 5]);
    client.unwatch(watch);
    daemon.push_event(append(&env, 1));
    quiet(&client, Duration::from_millis(300));
}
