//! TLS: only a certificate that chains to a trusted root and names exactly the dialled host gets a connection.

use std::io::Read;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Duration;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use skyelab_tailnet::Cert;

use super::*;
use crate::Error;

/// A TLS server on loopback serving `cert`. It records whether any handshake completed (so the client accepted
/// the certificate) and then hangs up, since it speaks no WebSocket.
struct RawTls {
    port: u16,
    accepted: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
}

fn raw_tls(cert: &Cert) -> RawTls {
    let chain = CertificateDer::pem_slice_iter(&cert.cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .expect("chain");
    let key = PrivateKeyDer::from_pem_slice(&cert.key_pem).expect("key");
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("tls13")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server cert"),
    );
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (accepted, connections) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicUsize::new(0)),
    );
    let (a, c) = (accepted.clone(), connections.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            c.fetch_add(1, Ordering::SeqCst);
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut conn = rustls::ServerConnection::new(config.clone()).expect("server conn");
            while conn.is_handshaking() {
                if conn.complete_io(&mut stream).is_err() {
                    break;
                }
            }
            if !conn.is_handshaking() {
                a.store(true, Ordering::SeqCst);
            }
        }
    });
    RawTls {
        port,
        accepted,
        connections,
    }
}

/// The client must never connect, and must say why: `want` appears in a Disconnected reason.
fn assert_refused(client: &Client, want: &str) -> String {
    match next(client) {
        Update::Disconnected(reason) => {
            assert!(reason.contains(want), "{want:?} not in {reason:?}");
            reason
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// Dials `host` at a raw TLS server serving `cert`; it must be refused for `want`, with no completed handshake.
fn refused_cert(cert: &Cert, host: &str, opts: impl FnOnce(u16) -> Options, want: &str) {
    let server = raw_tls(cert);
    let client = Client::connect(host, opts(server.port)).expect("client");
    assert_refused(&client, want);
    assert!(
        server.connections.load(Ordering::SeqCst) >= 1,
        "never dialled"
    );
    assert!(
        !server.accepted.load(Ordering::SeqCst),
        "the client finished a TLS handshake with a certificate it should refuse"
    );
}

#[test]
fn valid_connection_works() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    connected(&client);
    assert_eq!(client.list_projects().recv().expect("projects").len(), 1);
}

#[test]
fn accepted_cert_is_seen_by_the_raw_server() {
    // The harness itself: a good cert completes the handshake, so a refusal below means something.
    let server = raw_tls(&ca().issue(HOST));
    let client = Client::connect(HOST, options(server.port)).expect("client");
    assert_refused(&client, "WebSocket handshake failed");
    assert!(server.accepted.load(Ordering::SeqCst));
}

#[test]
fn wrong_name_is_refused() {
    refused_cert(
        &ca().issue("other.example.ts.net"),
        HOST,
        options,
        "not valid for name",
    );
}

#[test]
fn wildcard_cert_is_refused() {
    // The standard check accepts a wildcard; the exact-name rule must not.
    refused_cert(
        &ca().issue("*.example.ts.net"),
        HOST,
        options,
        "NotValidForName",
    );
}

#[test]
fn host_is_normalized() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let client =
        Client::connect("Desktop.EXAMPLE.ts.net.", options(port(&daemon))).expect("client");
    connected(&client);
}

#[test]
fn untrusted_ca_is_refused() {
    let other = TestCa::new();
    refused_cert(&other.issue(HOST), HOST, options, "UnknownIssuer");
}

#[test]
fn default_roots_refuse_test_ca() {
    // No override: a shipped client's trust store, which must not include the test CA.
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let opts = Options {
        roots: None,
        ..options(port(&daemon))
    };
    let client = Client::connect(HOST, opts).expect("client");
    assert_refused(&client, "UnknownIssuer");
    assert!(client.hello().is_none());
}

#[test]
fn expired_cert_is_refused() {
    refused_cert(
        &ca().issue_expired(HOST),
        HOST,
        options,
        "certificate expired",
    );
}

#[test]
fn plaintext_server_is_refused() {
    // A ws:// server: the client opens with a TLS ClientHello, never an HTTP upgrade, and never connects.
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let first = Arc::new(std::sync::Mutex::new(None::<u8>));
    let seen = first.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut byte = [0u8; 1];
            if stream.read_exact(&mut byte).is_ok() {
                seen.lock().expect("lock").get_or_insert(byte[0]);
            }
            // Answer like a plaintext WebSocket server would to garbage.
            let _ = std::io::Write::write_all(&mut stream, b"HTTP/1.1 400 Bad Request\r\n\r\n");
        }
    });
    let client = Client::connect(HOST, options(port)).expect("client");
    assert_refused(&client, "TLS");
    assert_eq!(
        *first.lock().expect("lock"),
        Some(0x16),
        "first byte must be a TLS handshake record"
    );
}

#[test]
fn ip_host_is_refused() {
    for host in [
        "100.64.0.1",
        "fd7a:115c:a1e0::1",
        "127.0.0.1",
        "[fd7a:115c:a1e0::1]",
        "",
    ] {
        let err = match Client::connect(host, options(1)) {
            Ok(_) => panic!("{host:?} accepted"),
            Err(e) => e,
        };
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::InvalidInput,
            "{host}: {err}"
        );
    }
}

#[test]
fn oversized_message_over_tls_closes() {
    let env = Env::new();
    env.append(200);
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    let opts = Options {
        max_message_bytes: 1024,
        ..options(port(&daemon))
    };
    let client = Client::connect(HOST, opts).expect("client");
    connected(&client);
    assert_eq!(
        client
            .thread_events_since(env.thread_id, 0)
            .recv_timeout(Duration::from_secs(5)),
        Err(Error::ConnectionLost)
    );
}

/// The real thing: this machine's `tailscale cert` output must verify with the shipped roots and the exact name.
/// `SKYELAB_LIVE_CERT_DIR=<dir with NAME.crt and NAME.key> SKYELAB_LIVE_NAME=<MagicDNS name>
/// cargo test -p skyelab-client live_cert_verifies -- --ignored --nocapture`
#[test]
#[ignore = "needs a real tailscale cert on this machine"]
fn live_cert_verifies() {
    let dir = std::env::var("SKYELAB_LIVE_CERT_DIR").expect("SKYELAB_LIVE_CERT_DIR");
    let name = std::env::var("SKYELAB_LIVE_NAME").expect("SKYELAB_LIVE_NAME");
    let read = |ext: &str| std::fs::read(format!("{dir}/{name}.{ext}")).expect("live cert file");
    let cert = Cert {
        cert_pem: read("crt"),
        key_pem: read("key"),
    };
    let server = raw_tls(&cert);
    let opts = Options {
        roots: None, // the shipped webpki roots
        ..options(server.port)
    };
    let client = Client::connect(&name, opts).expect("client");
    let reason = assert_refused(&client, "WebSocket handshake failed");
    println!(
        "live: TLS to {name} verified with webpki roots; then, as expected from a raw TLS server: {reason}"
    );
    assert!(server.accepted.load(Ordering::SeqCst));
    // And a different name on the same real cert is refused.
    let wrong = format!("x{name}");
    let server = raw_tls(&cert);
    let client = Client::connect(
        &wrong,
        Options {
            roots: None,
            ..options(server.port)
        },
    )
    .expect("client");
    let reason = assert_refused(&client, "not valid for name");
    println!("live: {wrong} on the same cert refused: {reason}");
}
