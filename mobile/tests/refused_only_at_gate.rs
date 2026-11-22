//! Refused means the daemon's gate closed the socket, not "any handshake failure".

pub mod common;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use common::options;
use skyelab_mobile::{
    remote::RemoteSource,
    source::{Connection, ThreadSource},
};

#[test]
fn a_plain_http_server_on_the_port_is_not_shown_as_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });

    let mut source = RemoteSource::connect("127.0.0.1", options(port)).expect("client thread");
    let inbox = source.inbox().expect("a remote source has an inbox");
    let deadline = Instant::now() + Duration::from_secs(5);
    while source.connection() == Connection::Connecting {
        let left = deadline.saturating_duration_since(Instant::now());
        let msg = inbox
            .recv_timeout(left)
            .expect("a connection update within 5 s");
        source.apply(msg);
    }
    assert_ne!(
        source.connection(),
        Connection::Refused,
        "an HTTP 404 from another server is not the daemon's gate"
    );
}
