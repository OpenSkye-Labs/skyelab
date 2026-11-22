//! A protocol mismatch is fatal and not retried.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::Error;
use skyelab_protocol::{VERSION, Version};

#[test]
fn version_mismatch_is_fatal() {
    let tailnet = Arc::new(FakeTailnet::default());
    let env = Env::new();
    let daemon = start_daemon(tailnet.clone(), env.backend(), 0);
    let opts = Options {
        protocol: Version {
            major: VERSION.major + 1,
            minor: 0,
        },
        ..options(port(&daemon))
    };
    let client = Client::connect(HOST, opts).expect("client");
    match next(&client) {
        Update::Fatal(Error::UnsupportedVersion(msg)) => {
            assert!(msg.contains("not supported"), "{msg}")
        }
        other => panic!("expected Fatal(UnsupportedVersion), got {other:?}"),
    }
    // Several backoff periods (50 ms doubling) pass without another attempt.
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(tailnet.whois_calls(), 1);
    assert_eq!(client.list_projects().recv(), Err(Error::Stopped));
}

#[test]
fn identity_is_checked_before_the_version() {
    // Another node answering with another major must not stop the client for good: it is refused and retried.
    use skyelab_protocol::{Hello, Reply, ServerMessage};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { return };
            let Ok(mut ws) = tungstenite::accept(stream) else {
                continue;
            };
            let _ = ws.read();
            let hello = ServerMessage::Response {
                id: 0,
                result: Reply::Hello(Hello {
                    protocol: Version {
                        major: VERSION.major + 1,
                        minor: 0,
                    },
                    capabilities: vec![],
                    node_id: Some("nImpostor".into()),
                }),
            };
            let _ = ws.send(tungstenite::Message::text(
                serde_json::to_string(&hello).expect("json"),
            ));
        }
    });
    let mut opts = options(port);
    opts.plaintext = true;
    opts.verify_peer = Some(crate::PeerVerifier::new(|_| Ok("nPinned".into())));
    let client = Client::connect(HOST, opts).expect("client");
    match next(&client) {
        Update::Disconnected(d) => {
            assert_eq!(d.kind, crate::DisconnectKind::IdentityMismatch, "{d:?}")
        }
        other => panic!("expected IdentityMismatch, got {other:?}"),
    }
}
