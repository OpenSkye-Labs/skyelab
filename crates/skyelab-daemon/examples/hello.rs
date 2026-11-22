//! Says `hello` to a running daemon and prints what happened: `cargo run -p skyelab-daemon --example hello -- <ip:port>`.
//! From the daemon's own machine this must be rejected: `is_own_device` refuses this node itself.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use skyelab_protocol::{Call, Reply, Request, ServerMessage, VERSION};
use tungstenite::Message;

fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: hello <ip:port>"))?
        .parse()?;
    let tcp = TcpStream::connect(addr)?;
    tcp.set_read_timeout(Some(Duration::from_secs(15)))?;
    println!("tcp connected to {addr}");
    let (mut ws, _) = match tungstenite::client(
        format!("ws://{addr}/"),
        tungstenite::stream::MaybeTlsStream::Plain(tcp),
    ) {
        Ok(ok) => ok,
        Err(e) => {
            println!("REJECTED before hello: handshake failed: {e}");
            return Ok(());
        }
    };
    let hello = Request {
        id: 1,
        call: Call::Hello {
            protocol: VERSION,
            capabilities: vec![],
        },
    };
    ws.send(Message::text(serde_json::to_string(&hello)?))?;
    match ws.read() {
        Ok(Message::Text(t)) => match serde_json::from_str::<ServerMessage>(&t) {
            Ok(ServerMessage::Response {
                result: Reply::Hello(h),
                ..
            }) => println!("ACCEPTED: hello {h:?}"),
            _ => println!("REJECTED at hello: {t}"),
        },
        Ok(other) => println!("REJECTED at hello: {other}"),
        Err(e) => println!("REJECTED after the handshake: {e}"),
    }
    Ok(())
}
