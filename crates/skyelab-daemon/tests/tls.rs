//! TLS: the daemon serves only TLS with this node's certificate, starts only with a good one, renews in place,
//! and still runs whois before reading a single byte.

mod common;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use common::*;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject as _;
use skyelab_daemon::testing::{NODE_NAME, ca};
use skyelab_daemon::{ADMIN_DNS_URL, Config, Daemon};
use skyelab_protocol::{Call, Reply, ServerMessage};
use skyelab_tailnet::Cert;
use tungstenite::stream::MaybeTlsStream;

fn free_port() -> u16 {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port()
}

/// Starting must fail with an error containing `expected`, and leave nothing listening.
fn assert_start_fails(tailnet: Arc<FakeTailnet>, expected: &[&str]) {
    let port = free_port();
    let config = Config {
        port,
        ..test_config()
    };
    let err = match Daemon::start(config, tailnet, Arc::new(Counting::default())) {
        Ok(_) => panic!("started without a usable certificate"),
        Err(e) => format!("{e:#}"),
    };
    for want in expected {
        assert!(err.contains(want), "{want:?} missing from: {err}");
    }
    assert!(
        std::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err(),
        "something listens on {port} after a failed start"
    );
}

fn leaf(cert: &Cert) -> CertificateDer<'static> {
    CertificateDer::pem_slice_iter(&cert.cert_pem)
        .next()
        .expect("a leaf")
        .expect("valid pem")
}

fn served_leaf(ws: &Client) -> CertificateDer<'static> {
    ws.get_ref().conn.peer_certificates().expect("peer certs")[0].clone()
}

/// A real TLS 1.3 ClientHello for NODE_NAME.
fn client_hello() -> Vec<u8> {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca().root()).expect("root");
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("tls13")
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut conn =
        rustls::ClientConnection::new(Arc::new(config), NODE_NAME.try_into().expect("name"))
            .expect("conn");
    let mut hello = Vec::new();
    conn.write_tls(&mut hello).expect("hello");
    hello
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < until, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn plaintext_client_is_refused() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    let addr = daemon.local_addrs()[0];
    let result = tungstenite::client(format!("ws://{addr}/"), MaybeTlsStream::Plain(tcp(addr)));
    assert!(result.is_err(), "a ws:// handshake must not complete");
    assert_eq!(tailnet.whois_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn rejected_peer_gets_no_tls() {
    let tailnet = FakeTailnet::new(Peer::OtherUser);
    let backend = Arc::new(Counting::default());
    let daemon = start(tailnet.clone(), backend.clone(), test_config());
    let mut sock = tcp(daemon.local_addrs()[0]);
    sock.write_all(&client_hello()).expect("send hello");
    let mut buf = [0u8; 512];
    // EOF or a reset, but never a ServerHello: TLS never starts for a peer whois refused.
    match sock.read(&mut buf) {
        Ok(n) => assert_eq!(n, 0, "the daemon answered a refused peer: {:?}", &buf[..n]),
        Err(e) => assert_ne!(e.kind(), std::io::ErrorKind::WouldBlock, "no close: {e}"),
    }
    assert_eq!(tailnet.whois_calls.load(Ordering::SeqCst), 1);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    assert!(try_connect(daemon.local_addrs()[0]).is_err());
}

#[test]
fn stalled_tls_handshake_is_dropped() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    let started = Instant::now();
    let mut sock = tcp(daemon.local_addrs()[0]);
    let hello = client_hello();
    sock.write_all(&hello[..5]).expect("half a hello");
    let mut buf = [0u8; 64];
    let n = sock.read(&mut buf).unwrap_or(0);
    assert_eq!(n, 0, "expected a close");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "a stalled handshake held the connection for {:?}",
        started.elapsed()
    );
    // The slot is free: a real client still gets in.
    connect_hello(daemon.local_addrs()[0]);
}

#[test]
fn start_fails_without_magicdns() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.dns_name.lock().expect("lock") = String::new();
    assert_start_fails(tailnet.clone(), &["MagicDNS is off", ADMIN_DNS_URL]);
    assert_eq!(tailnet.cert_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn start_fails_when_cert_errors() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.cert.lock().expect("lock") = Err("HTTPS certificates are off".into());
    assert_start_fails(tailnet, &[NODE_NAME, "HTTPS certificates are off"]);
}

#[test]
fn start_fails_on_wrong_name_cert() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.cert.lock().expect("lock") = Ok(ca().issue("other.example.ts.net"));
    assert_start_fails(
        tailnet,
        &["other.example.ts.net", "not desktop.example.ts.net"],
    );
}

#[test]
fn start_fails_on_wildcard_cert() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.cert.lock().expect("lock") = Ok(ca().issue("*.example.ts.net"));
    assert_start_fails(tailnet, &["not desktop.example.ts.net"]);
}

#[test]
fn start_fails_on_expired_cert() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.cert.lock().expect("lock") = Ok(ca().issue_expired(NODE_NAME));
    assert_start_fails(tailnet, &["expired"]);
}

#[test]
fn start_fails_on_mismatched_key() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let (a, b) = (ca().issue(NODE_NAME), ca().issue(NODE_NAME));
    *tailnet.cert.lock().expect("lock") = Ok(Cert {
        cert_pem: a.cert_pem,
        key_pem: b.key_pem,
    });
    assert_start_fails(tailnet, &["doesn't match"]);
}

#[test]
fn certificate_reports_name_and_expiry() {
    let tailnet = FakeTailnet::new(Peer::Own);
    *tailnet.cert.lock().expect("lock") = Ok(ca().issue_until(&[NODE_NAME], (2098, 3, 4)));
    let daemon = start(tailnet, Arc::new(Counting::default()), test_config());
    let info = daemon.certificate();
    assert_eq!(info.name, NODE_NAME);
    assert_eq!(info.expires_utc(), "2098-03-04 00:00 UTC");
}

fn renewing_config() -> Config {
    Config {
        cert_check_every: Duration::from_millis(100),
        cert_retry: Duration::from_millis(100),
        ..test_config()
    }
}

#[test]
fn renewal_swaps_cert() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let first = ca().issue_until(&[NODE_NAME], (2097, 1, 1));
    *tailnet.cert.lock().expect("lock") = Ok(first.clone());
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        renewing_config(),
    );
    let addr = daemon.local_addrs()[0];
    let mut old = connect_hello(addr);
    assert_eq!(served_leaf(&old), leaf(&first));

    let second = ca().issue_until(&[NODE_NAME], (2098, 1, 1));
    *tailnet.cert.lock().expect("lock") = Ok(second.clone());
    wait_for("the renewed cert", || {
        daemon.certificate().expires_utc().starts_with("2098-01-01")
    });
    let new = connect_hello(addr);
    assert_eq!(served_leaf(&new), leaf(&second));
    assert_ne!(leaf(&first), leaf(&second));

    // The connection made before the renewal keeps working.
    send(&mut old, 2, Call::ListProjects);
    assert!(matches!(
        recv(&mut old),
        Some(ServerMessage::Response {
            id: 2,
            result: Reply::Projects(_)
        })
    ));
}

#[test]
fn failed_renewal_keeps_old_cert() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let first = ca().issue_until(&[NODE_NAME], (2097, 1, 1));
    *tailnet.cert.lock().expect("lock") = Ok(first.clone());
    let daemon = start(
        tailnet.clone(),
        Arc::new(Counting::default()),
        renewing_config(),
    );
    let addr = daemon.local_addrs()[0];
    let before = daemon.certificate();
    for bad in [
        Err("tailscale cert failed".to_owned()),
        Ok(ca().issue_expired(NODE_NAME)),
        Ok(ca().issue("other.example.ts.net")),
        Ok(Cert {
            cert_pem: b"garbage".to_vec(),
            key_pem: b"garbage".to_vec(),
        }),
    ] {
        *tailnet.cert.lock().expect("lock") = bad;
        let calls = tailnet.cert_calls.load(Ordering::SeqCst);
        wait_for("two more renewal attempts", || {
            tailnet.cert_calls.load(Ordering::SeqCst) >= calls + 2
        });
        assert_eq!(daemon.certificate(), before);
        assert_eq!(served_leaf(&connect_hello(addr)), leaf(&first));
    }
}

#[test]
fn hung_renewal_doesnt_block_the_gate() {
    let tailnet = FakeTailnet::new(Peer::Own);
    let config = Config {
        cert_check_every: Duration::from_millis(50),
        ..test_config()
    };
    let daemon = start(tailnet.clone(), Arc::new(Counting::default()), config);
    *tailnet.cert_hang.lock().expect("lock") = Duration::from_secs(5);
    wait_for("a renewal to start", || {
        tailnet.cert_calls.load(Ordering::SeqCst) >= 2
    });
    let started = Instant::now();
    connect_hello(daemon.local_addrs()[0]);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the gate waited {:?} behind a hung renewal",
        started.elapsed()
    );
}
