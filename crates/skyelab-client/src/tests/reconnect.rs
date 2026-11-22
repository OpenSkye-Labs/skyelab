//! The server goes away and comes back; the client reconnects and resyncs.

use std::sync::Arc;
use std::time::Duration;

use super::*;

#[test]
fn reconnects_and_fills_what_it_missed() {
    let env = Env::new();
    append(&env, 2);
    let tailnet = Arc::new(FakeTailnet::default());
    let daemon = start_daemon(tailnet.clone(), env.backend(), 0);
    let port = port(&daemon);
    let client = Client::connect(HOST, options(port)).expect("client");
    connected(&client);
    client.watch(env.thread_id, 0);
    assert_eq!(seqs(&client, 2), [1, 2]);

    drop(daemon);
    match next(&client) {
        Update::Disconnected(_) => {}
        other => panic!("expected Disconnected, got {other:?}"),
    }
    assert!(client.hello().is_none());
    // Written while the client was away, never pushed.
    append(&env, 3);
    std::thread::sleep(Duration::from_millis(200));

    let daemon = start_daemon(tailnet, env.backend(), port);
    loop {
        match next(&client) {
            Update::Connected(_) => break,
            Update::Disconnected(_) => continue, // attempts while the port was closed
            other => panic!("expected Connected, got {other:?}"),
        }
    }
    assert_eq!(seqs(&client, 3), [3, 4, 5]);
    daemon.push_event(append(&env, 1));
    assert_eq!(seqs(&client, 1), [6]);
    assert_eq!(client.list_projects().recv().expect("projects").len(), 1);
    quiet(&client, Duration::from_millis(300));
}
