//! With `approve_each_connection`, a connection that passed hello sends nothing (no watch refill, no request) and
//! delivers no event until the caller approves that hello. Every reconnect is held again.

use std::time::Duration;

use super::*;

fn held_options(port: u16) -> Options {
    let mut options = options(port);
    options.approve_each_connection = true;
    options
}

fn hello_of(update: Update) -> skyelab_protocol::Hello {
    match update {
        Update::Connected(hello) => hello,
        other => panic!("expected Connected, got {other:?}"),
    }
}

#[test]
fn nothing_reaches_the_server_until_the_hello_is_approved() {
    let mut env = Env::new();
    append(&env, 2);
    env.serve();
    let client = Client::connect(HOST, held_options(env.port)).expect("client");
    let _watch = client.watch(env.thread_id, 0);
    let hello = hello_of(next(&client));

    // Held: a pushed event is not delivered, a request fails, and the server is asked nothing.
    if let Some(daemon) = &env.daemon {
        daemon.push_event(append(&env, 1));
    }
    assert_eq!(
        client.list_projects().recv_timeout(Duration::from_secs(60)),
        Err(crate::Error::NotConnected)
    );
    quiet(&client, Duration::from_millis(300));
    assert!(env.asked().is_empty(), "served {:?}", env.asked());

    // Approved: the watch refills, requests go out.
    client.approve(&hello);
    assert_eq!(seqs(&client, 3), [1, 2, 3]);
    assert_eq!(client.list_projects().recv().expect("projects").len(), 1);
    assert!(env.asked().contains(&"thread_events_since"));

    // A reconnect is held again.
    env.stop();
    match next(&client) {
        Update::Disconnected(_) => {}
        other => panic!("expected Disconnected, got {other:?}"),
    }
    append(&env, 1);
    env.asked_log.lock().expect("asked lock").clear();
    env.serve();
    let hello = loop {
        match next(&client) {
            Update::Connected(hello) => break hello,
            Update::Disconnected(_) => continue,
            other => panic!("expected Connected, got {other:?}"),
        }
    };
    quiet(&client, Duration::from_millis(300));
    assert!(env.asked().is_empty(), "served {:?}", env.asked());
    client.approve(&hello);
    assert_eq!(seqs(&client, 1), [4]);
}

#[test]
fn approving_another_hello_does_not_release_the_connection() {
    let mut env = Env::new();
    append(&env, 1);
    env.serve();
    let client = Client::connect(HOST, held_options(env.port)).expect("client");
    let _watch = client.watch(env.thread_id, 0);
    let mut other = hello_of(next(&client));
    other.node_id = Some("nSomeoneElse".into());
    client.approve(&other);
    quiet(&client, Duration::from_millis(300));
    assert!(env.asked().is_empty(), "served {:?}", env.asked());
}
