//! Port forwarding (1.5) against the real daemon: bytes reach a loopback server there through a 127.0.0.1 port
//! here, and the forward stops when dropped or when the connection drops.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use skyelab_protocol::{Hello, Reply, Request, ServerMessage, Version};
use tungstenite::Message;

use super::*;
use crate::{Error, ErrorCode, Forward};

/// An echo server on 127.0.0.1, standing in for an agent's dev server on the other machine.
fn echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut conn = conn;
                let mut buf = [0u8; 4096];
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

fn local(forward: &Forward) -> TcpStream {
    let tcp =
        TcpStream::connect((Ipv4Addr::LOCALHOST, forward.local_port)).expect("connect locally");
    tcp.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    tcp
}

fn echo(tcp: &mut TcpStream, bytes: &[u8]) -> Vec<u8> {
    tcp.write_all(bytes).expect("write");
    let mut got = vec![0u8; bytes.len()];
    tcp.read_exact(&mut got).expect("read back");
    got
}

/// True once the local connection was closed (EOF or reset), false on a read timeout.
fn closed(tcp: &mut TcpStream) -> bool {
    let mut buf = [0u8; 16];
    match tcp.read(&mut buf) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => !matches!(
            e.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

/// True once nothing accepts on the forward's local port, within 5 s.
fn listener_gone(port: u16) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn up(env: &Env) -> Client {
    let client = Client::connect(HOST, options(env.port)).expect("client");
    connected(&client);
    client
}

#[test]
fn a_forward_carries_bytes_through_a_loopback_port() {
    // A slow whois, so tunnels being dialled overlap at the daemon's gate as they do over a real tailnet.
    let mut env = Env::with(
        FakeTailnet {
            peer: std::sync::Mutex::new(Peer::Hang(Duration::from_millis(100))),
            ..FakeTailnet::default()
        },
        "/src/app",
        "fix it",
    );
    env.serve();
    let remote = echo_server();
    let client = up(&env);
    let forward = client.forward_port(remote).recv().expect("forwarded");
    assert_eq!(forward.remote_port, remote);
    // The echo server holds that port on 127.0.0.1 here too, so another one was picked.
    assert_ne!(forward.local_port, remote);
    let mut tcp = local(&forward);
    assert_eq!(
        echo(&mut tcp, b"GET / HTTP/1.1\r\n\r\n"),
        b"GET / HTTP/1.1\r\n\r\n"
    );
    // A browser opens several at once: none is dropped by the daemon's per-address gate limit.
    let mut many: Vec<TcpStream> = (0..8).map(|_| local(&forward)).collect();
    for (i, tcp) in many.iter_mut().enumerate() {
        let msg = format!("conn {i}");
        assert_eq!(echo(tcp, msg.as_bytes()), msg.as_bytes());
    }
    // The same port can't be forwarded twice.
    match client.forward_port(remote).recv() {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::NotWired),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_refused_port_is_an_error_and_can_be_retried() {
    let env = Env::start();
    let client = up(&env);
    match client.forward_port(22).recv() {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::BadRequest),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let free = TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("addr")
        .port();
    match client.forward_port(free).recv() {
        Err(Error::Rpc(e)) => assert_eq!(e.code, ErrorCode::NotFound),
        other => panic!("expected nothing listening, got {other:?}"),
    }
}

#[test]
fn dropping_the_forward_stops_it_here_and_there() {
    let env = Env::start();
    let remote = echo_server();
    let client = up(&env);
    let forward = client.forward_port(remote).recv().expect("forwarded");
    let port = forward.local_port;
    let mut tcp = local(&forward);
    assert_eq!(echo(&mut tcp, b"hi"), b"hi");
    drop(forward);
    assert!(closed(&mut tcp), "the open tunnel outlived its forward");
    assert!(
        listener_gone(port),
        "the local listener outlived its forward"
    );
    // The environment was told: the port can be forwarded again.
    let again = client.forward_port(remote).recv().expect("forwarded again");
    assert_eq!(echo(&mut local(&again), b"back"), b"back");
}

#[test]
fn a_dropped_connection_stops_the_forward() {
    let mut env = Env::start();
    let remote = echo_server();
    let client = up(&env);
    let forward = client.forward_port(remote).recv().expect("forwarded");
    let mut tcp = local(&forward);
    assert_eq!(echo(&mut tcp, b"hi"), b"hi");
    env.stop();
    assert!(closed(&mut tcp), "the tunnel outlived the connection");
    assert!(
        listener_gone(forward.local_port),
        "the listener outlived the connection"
    );
}

#[test]
fn the_local_listener_binds_loopback_only() {
    let busy = TcpListener::bind("127.0.0.1:0").expect("bind");
    let taken = busy.local_addr().expect("addr").port();
    let listener = crate::conn::forward::bind_local(taken).expect("bind");
    let addr = listener.local_addr().expect("addr");
    assert_eq!(addr.ip(), std::net::IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(addr.port(), taken, "a taken port falls back to a free one");
    drop(busy);
    let free = listener.local_addr().expect("addr").port();
    drop(listener);
    let again = crate::conn::forward::bind_local(free).expect("bind");
    assert_eq!(
        again.local_addr().expect("addr"),
        SocketAddr::from((Ipv4Addr::LOCALHOST, free)),
        "a free port is kept"
    );
}

/// An environment older than 1.5 is never sent `forward_port`.
#[test]
fn an_older_environment_is_not_asked() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let (seen_tx, seen) = flume::unbounded::<String>();
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut ws) = tungstenite::accept(stream) else {
            return;
        };
        let Ok(Message::Text(hello)) = ws.read() else {
            return;
        };
        let id = serde_json::from_str::<Request>(&hello).expect("hello").id;
        let reply = ServerMessage::Response {
            id,
            result: Reply::Hello(Hello {
                protocol: Version { major: 1, minor: 4 },
                capabilities: vec![],
                node_id: None,
            }),
        };
        ws.send(Message::text(reply.to_json())).expect("send hello");
        while let Ok(msg) = ws.read() {
            if let Message::Text(t) = msg {
                let _ = seen_tx.send(t.to_string());
            }
        }
    });
    let client = Client::connect(
        HOST,
        Options {
            plaintext: true,
            ..options(port)
        },
    )
    .expect("client");
    connected(&client);
    match client.forward_port(5173).recv() {
        Err(Error::Rpc(e)) => {
            assert_eq!(e.code, ErrorCode::NotWired);
            assert!(e.message.contains("older"), "{e:?}");
        }
        other => panic!("expected too old, got {other:?}"),
    }
    assert!(
        seen.recv_timeout(Duration::from_millis(300)).is_err(),
        "nothing was sent"
    );
}
