//! Port forwarding (1.5): a tunnel reaches a loopback port only while a live connection of the same approved
//! device forwards it, never a refused port, and it ends with that forward.

mod common;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;

use common::*;
use skyelab_daemon::Approval;
use skyelab_daemon::testing::{Peer, approvals};
use skyelab_protocol::{Call, ErrorCode, Reply, ServerMessage};
use tungstenite::Message;

const OTHER_KEY: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

/// An echo server on 127.0.0.1, standing in for a dev server on the serving machine.
fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("test setup");
    let port = listener.local_addr().expect("test setup").port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut conn = conn;
                let mut buf = [0u8; 1024];
                while let Ok(n) = conn.read(&mut buf) {
                    if n == 0 || conn.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

/// A port nothing listens on: bound, then let go.
fn closed_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("test setup");
    l.local_addr().expect("test setup").port()
}

fn daemon() -> skyelab_daemon::Daemon {
    let mut config = test_config();
    config.approved = approvals();
    config.approved.push(Approval {
        node_id: "nPhone".into(),
        key_hash: skyelab_protocol::device_key_hash(OTHER_KEY),
    });
    start(
        FakeTailnet::new(Peer::Own),
        Arc::new(Counting::default()),
        config,
    )
}

fn addr(d: &skyelab_daemon::Daemon) -> SocketAddr {
    d.local_addrs()[0]
}

fn is_ok(msg: Option<ServerMessage>) -> bool {
    matches!(
        msg,
        Some(ServerMessage::Response {
            result: Reply::Ok,
            ..
        })
    )
}

/// `forward_port`: the forward's token, or the error code.
fn forward(ws: &mut Client, port: u16) -> Result<String, ErrorCode> {
    send(ws, 2, Call::ForwardPort { port });
    match recv(ws) {
        Some(ServerMessage::Response {
            result: Reply::Forwarded { token },
            ..
        }) => Ok(token),
        msg => Err(error_code(msg)),
    }
}

/// A connection that said hello as a tunnel.
fn tunnel_hello(d: &skyelab_daemon::Daemon) -> Client {
    let mut ws = connect(addr(d));
    send(
        &mut ws,
        1,
        Call::Hello {
            protocol: skyelab_protocol::VERSION,
            capabilities: vec![skyelab_protocol::TUNNEL_CAPABILITY.into()],
            device_key: Some(KEY.into()),
        },
    );
    assert!(matches!(
        recv(&mut ws),
        Some(ServerMessage::Response { id: 1, .. })
    ));
    ws
}

fn open(ws: &mut Client, port: u16, token: &str) -> Option<ServerMessage> {
    let token = token.to_owned();
    send(ws, 2, Call::OpenTunnel { port, token });
    recv(ws)
}

fn tunnel(d: &skyelab_daemon::Daemon, port: u16, token: &str) -> Result<Client, ErrorCode> {
    let mut ws = tunnel_hello(d);
    match open(&mut ws, port, token) {
        msg if is_ok(msg.clone()) => Ok(ws),
        msg => Err(error_code(msg)),
    }
}

fn round_trip(ws: &mut Client, bytes: &[u8]) -> Vec<u8> {
    ws.send(Message::binary(bytes.to_vec())).expect("send");
    loop {
        match ws.read().expect("read") {
            Message::Binary(b) => return b.to_vec(),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected bytes, got {other:?}"),
        }
    }
}

/// True once the server ended the tunnel (close frame or EOF), not on a read timeout.
fn ended(ws: &mut Client) -> bool {
    loop {
        match ws.read() {
            Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(Message::Close(_)) => return true,
            Ok(_) => return false,
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                return false;
            }
            Err(_) => return true,
        }
    }
}

#[test]
fn a_forwarded_port_carries_bytes_both_ways() {
    let d = daemon();
    let port = echo_server();
    let mut owner = connect_hello(addr(&d));
    let token = forward(&mut owner, port).expect("forwarded");
    let mut ws = tunnel(&d, port, &token).expect("tunnel admitted");
    assert_eq!(
        round_trip(&mut ws, b"GET / HTTP/1.1\r\n\r\n"),
        b"GET / HTTP/1.1\r\n\r\n"
    );
    assert_eq!(round_trip(&mut ws, b"again"), b"again");
    // The tunnel isn't a device of its own in Settings.
    assert_eq!(d.peers().len(), 1, "{:?}", d.peers());
}

#[test]
fn refuses_system_own_denied_and_silent_ports() {
    let d = daemon();
    let mut owner = connect_hello(addr(&d));
    for port in [22, 80, addr(&d).port(), 9229, 2375, 5900, 5037, 27017] {
        assert_eq!(
            forward(&mut owner, port),
            Err(ErrorCode::BadRequest),
            "port {port}"
        );
    }
    assert_eq!(forward(&mut owner, closed_port()), Err(ErrorCode::NotFound));
}

#[test]
fn a_tunnel_needs_a_forward_by_the_same_device() {
    let d = daemon();
    let port = echo_server();
    // Nobody asked for it.
    assert_eq!(
        tunnel(&d, port, "made-up").err(),
        Some(ErrorCode::BadRequest)
    );
    // Another approved key on the same node asked; this key didn't, even with that forward's token.
    let mut other = connect(addr(&d));
    assert!(matches!(
        hello_key(&mut other, Some(OTHER_KEY)),
        Some(ServerMessage::Response { .. })
    ));
    let theirs = forward(&mut other, port).expect("forwarded");
    assert_eq!(tunnel(&d, port, &theirs).err(), Some(ErrorCode::BadRequest));
    // This key forwards it too: only its own token opens a tunnel.
    let mut owner = connect_hello(addr(&d));
    let mine = forward(&mut owner, port).expect("forwarded");
    assert_ne!(mine, theirs);
    assert_eq!(
        tunnel(&d, port, "made-up").err(),
        Some(ErrorCode::BadRequest)
    );
    assert!(tunnel(&d, port, &mine).is_ok());
    // Before hello it is refused like any call.
    let mut fresh = connect(addr(&d));
    send(
        &mut fresh,
        1,
        Call::OpenTunnel {
            port,
            token: mine.clone(),
        },
    );
    assert_eq!(error_code(recv(&mut fresh)), ErrorCode::HelloRequired);
    // A connection that didn't say hello as a tunnel can't become one.
    let mut plain = connect_hello(addr(&d));
    assert_eq!(
        error_code(open(&mut plain, port, &mine)),
        ErrorCode::BadRequest
    );
}

#[test]
fn a_tunnel_connection_does_nothing_else() {
    let d = daemon();
    let port = echo_server();
    let mut ws = tunnel_hello(&d);
    // Not listed, so it can't be allowed to forward or drive anything out of Disconnect's reach.
    assert!(d.peers().is_empty(), "{:?}", d.peers());
    send(&mut ws, 2, Call::ForwardPort { port });
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::BadRequest);
    assert!(ended(&mut ws), "closed after the refusal");
    let mut ws = tunnel_hello(&d);
    send(&mut ws, 2, Call::ListProjects);
    assert_eq!(error_code(recv(&mut ws)), ErrorCode::BadRequest);
}

#[test]
fn a_tunnel_ends_when_its_owner_closes() {
    let d = daemon();
    let port = echo_server();
    let mut owner = connect_hello(addr(&d));
    let token = forward(&mut owner, port).expect("forwarded");
    let mut ws = tunnel(&d, port, &token).expect("tunnel");
    assert_eq!(round_trip(&mut ws, b"hi"), b"hi");
    drop(owner);
    assert!(ended(&mut ws), "the tunnel outlived its forward");
    assert_eq!(tunnel(&d, port, &token).err(), Some(ErrorCode::BadRequest));
}

#[test]
fn stop_forward_ends_its_tunnels() {
    let d = daemon();
    let port = echo_server();
    let mut owner = connect_hello(addr(&d));
    let token = forward(&mut owner, port).expect("forwarded");
    let mut ws = tunnel(&d, port, &token).expect("tunnel");
    // A stale token stops nothing.
    let stale = "stale".to_owned();
    send(&mut owner, 3, Call::StopForward { port, token: stale });
    assert!(is_ok(recv(&mut owner)));
    assert_eq!(round_trip(&mut ws, b"still"), b"still");
    send(
        &mut owner,
        4,
        Call::StopForward {
            port,
            token: token.clone(),
        },
    );
    assert!(is_ok(recv(&mut owner)));
    assert!(ended(&mut ws));
    assert_eq!(tunnel(&d, port, &token).err(), Some(ErrorCode::BadRequest));
}

#[test]
fn a_revoke_ends_the_tunnel() {
    let d = daemon();
    let port = echo_server();
    let mut owner = connect_hello(addr(&d));
    let token = forward(&mut owner, port).expect("forwarded");
    let mut ws = tunnel(&d, port, &token).expect("tunnel");
    d.set_approved(vec![]);
    assert!(ended(&mut ws));
}
