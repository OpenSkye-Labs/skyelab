//! Why a connection failed, typed: the remote's gate turning this device away (`Refused`) is told apart from a
//! certificate problem and from everything else, without matching error text.

use std::io::Write as _;
use std::net::{Ipv4Addr, TcpListener};
use std::sync::Arc;

use super::*;
use crate::{Disconnect, DisconnectKind};

fn disconnect(client: &Client) -> Disconnect {
    match next(client) {
        Update::Disconnected(d) => d,
        other => panic!("expected Disconnected, got {other:?}"),
    }
}

/// A TCP server that answers each connection with `reply` (maybe nothing) and hangs up.
fn hang_up_with(reply: &'static [u8]) -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let _ = stream.write_all(reply);
            // Hang up the way a server does: read what the client sent first. Closing with its bytes unread
            // resets the connection, and Windows then drops `reply` before the client reads it.
            let _ = stream.shutdown(std::net::Shutdown::Write);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let _ = std::io::copy(&mut stream, &mut std::io::sink());
        }
    });
    port
}

#[test]
fn the_gate_turning_this_device_away_is_a_refusal() {
    let env = Env::new();
    let tailnet = Arc::new(FakeTailnet::default());
    tailnet.set_peer(Peer::WhoisError);
    let daemon = start_daemon(tailnet, env.backend(), 0);
    let client = Client::connect(HOST, options(port(&daemon))).expect("client");
    let d = disconnect(&client);
    assert_eq!(d.kind, DisconnectKind::Refused, "{d:?}");
    assert!(!d.message.is_empty());
    assert_eq!(d.to_string(), d.message, "Display is the message");
}

#[test]
fn a_peer_that_hangs_up_before_any_byte_is_a_refusal() {
    let client = Client::connect(HOST, options(hang_up_with(b""))).expect("client");
    assert_eq!(disconnect(&client).kind, DisconnectKind::Refused);
}

#[test]
fn a_peer_that_answers_then_hangs_up_is_not_a_refusal() {
    // Bytes came back, so the gate let us through: whatever broke after that is something else.
    let port = hang_up_with(b"HTTP/1.1 400 Bad Request\r\n\r\n");
    let client = Client::connect(HOST, options(port)).expect("client");
    let d = disconnect(&client);
    assert_eq!(d.kind, DisconnectKind::Other, "{d:?}");
}

#[test]
fn a_certificate_for_another_name_is_a_certificate_problem() {
    let env = Env::new();
    let daemon = start_daemon(Arc::new(FakeTailnet::default()), env.backend(), 0);
    // The daemon's certificate names HOST, not this.
    let client = Client::connect("other.example.ts.net", options(port(&daemon))).expect("client");
    let d = disconnect(&client);
    assert_eq!(d.kind, DisconnectKind::Certificate, "{d:?}");
    assert!(d.message.contains("certificate"), "{d:?}");
}

#[test]
fn nothing_listening_is_other() {
    let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = free.local_addr().expect("addr").port();
    drop(free);
    let client = Client::connect(HOST, options(port)).expect("client");
    assert_eq!(disconnect(&client).kind, DisconnectKind::Other);
}

#[test]
fn plaintext_peer_hanging_up_before_any_byte_is_a_refusal() {
    let mut opts = options(hang_up_with(b""));
    opts.plaintext = true;
    let client = Client::connect(HOST, opts).expect("client");
    assert_eq!(disconnect(&client).kind, DisconnectKind::Refused);
}

#[test]
fn a_refused_peer_gets_not_a_single_byte() {
    use std::io::Read as _;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (got_tx, got) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf); // until the client hangs up
        let _ = got_tx.send(buf.len());
    });
    let seen = Arc::new(std::sync::Mutex::new(None));
    let saw = seen.clone();
    let mut opts = options(port);
    opts.verify_peer = Some(crate::PeerVerifier::new(move |peer| {
        *saw.lock().expect("lock") = Some(peer);
        Err("not yours".into())
    }));
    let client = Client::connect(HOST, opts).expect("client");
    let d = disconnect(&client);
    assert_eq!(d.kind, DisconnectKind::NotYourDevice, "{d:?}");
    assert!(d.message.contains("not yours"), "{d:?}");
    let peer = seen.lock().expect("lock").expect("verifier ran");
    assert_eq!(peer.port(), port, "it checks the socket's own peer address");
    drop(client);
    let bytes = got
        .recv_timeout(Duration::from_secs(60))
        .expect("server saw the hang-up");
    assert_eq!(
        bytes, 0,
        "nothing (not even a TLS hello) went to a refused peer"
    );
}
