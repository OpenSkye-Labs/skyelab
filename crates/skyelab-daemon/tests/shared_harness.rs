//! Acceptance: the one fake-tailnet harness every crate's remote tests share (`skyelab_daemon::testing`), and
//! `skyelab_client::testing::options` reaching it over loopback TLS.

use std::time::Duration;

use skyelab_daemon::testing::{Env, FakeTailnet, ME, NODE_NAME, Peer, Ran, ca, node};
use skyelab_protocol::ErrorCode;
use skyelab_tailnet::Tailnet;

#[test]
fn fake_tailnet_is_configurable() {
    let tailnet = FakeTailnet {
        peers: vec![node("nLaptop", "laptop.example.ts.net", ME, true, &[])],
        ..FakeTailnet::default()
    };
    let me = tailnet.self_node().expect("self node");
    assert_eq!(me.dns_name, format!("{NODE_NAME}."));
    assert_eq!(me.user_id, Some(ME));
    assert_eq!(
        tailnet.peers().expect("peers")[0].dns_name,
        "laptop.example.ts.net."
    );

    let addr = "127.0.0.1:1".parse().expect("addr");
    let own = tailnet.whois(addr).expect("own device");
    assert_eq!((own.node.user, own.user_profile.id), (Some(ME), Some(ME)));
    assert!(own.node.tags.is_empty());
    tailnet.set_peer(Peer::Tagged);
    assert_eq!(
        tailnet.whois(addr).expect("tagged").node.tags,
        ["tag:server"]
    );
    tailnet.set_peer(Peer::OtherUser);
    assert_ne!(tailnet.whois(addr).expect("other").node.user, Some(ME));
    tailnet.set_peer(Peer::WhoisError);
    assert!(tailnet.whois(addr).is_err());
    assert_eq!(tailnet.whois_calls(), 4);

    // The cert comes from the shared test CA and names this node.
    let cert = tailnet.cert(NODE_NAME).expect("cert");
    assert!(!cert.cert_pem.is_empty() && !cert.key_pem.is_empty());
    let _ = ca();
}

#[test]
fn env_serves_its_store_and_records_agent_calls() {
    let mut env = Env::start();
    let client = skyelab_client::Client::connect(
        NODE_NAME,
        skyelab_client::testing::options(env.port, ca().root()),
    )
    .expect("client");
    match client.updates().recv_timeout(Duration::from_secs(5)) {
        Ok(skyelab_client::Update::Connected(_)) => {}
        other => panic!("expected Connected, got {other:?}"),
    }
    let projects = client.list_projects().recv().expect("projects");
    assert_eq!(
        (projects[0].id, projects[0].name.as_str()),
        (env.project_id, "app")
    );
    let threads = client.list_threads(env.project_id).recv().expect("threads");
    assert_eq!(
        (threads[0].id, threads[0].title.as_str()),
        (env.thread_id, "fix it")
    );

    client
        .send_message(env.thread_id, "go")
        .recv()
        .expect("send");
    client.interrupt(env.thread_id).recv().expect("interrupt");
    let started = client
        .start_thread(env.project_id, "new one")
        .recv()
        .expect("start");
    assert_eq!(
        env.ran(),
        [
            Ran::Send(env.thread_id, "go".into()),
            Ran::Interrupt(env.thread_id),
            Ran::Start(env.project_id, "new one".into()),
        ]
    );
    // A started thread is a real row with its prompt stored, so it can be listed and watched.
    let events = client
        .thread_events_since(started, 0)
        .recv()
        .expect("events");
    assert_eq!(events[0].kind, "prompt");
    assert_eq!(events[0].payload_json, "\"new one\"");

    env.agents
        .fail
        .store(true, std::sync::atomic::Ordering::SeqCst);
    match client.send_message(env.thread_id, "again").recv() {
        Err(skyelab_client::Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::NotWired),
        other => panic!("expected NotWired, got {other:?}"),
    }

    let pushed = env.push("prompt", "\"live\"");
    assert_eq!(pushed.thread_id, env.thread_id);

    // Stop and serve again on the same port.
    let port = env.port;
    env.stop();
    assert!(env.daemon.is_none());
    env.serve();
    assert_eq!(
        env.daemon.as_ref().expect("serving").local_addrs()[0].port(),
        port
    );
}
