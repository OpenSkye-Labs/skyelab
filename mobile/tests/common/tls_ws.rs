//! Accepts one loopback TLS 1.3 + WebSocket connection with the test CA's cert for `NODE_NAME`, so mock protocol
//! servers work with `common::options(port)` over real `wss://`.

use std::net::TcpStream;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use skyelab_daemon::testing::{NODE_NAME, ca};
use tungstenite::WebSocket;

pub fn accept(stream: TcpStream) -> Option<WebSocket<StreamOwned<ServerConnection, TcpStream>>> {
    let cert = ca().issue(NODE_NAME);
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert.cert_pem)
        .collect::<Result<_, _>>()
        .expect("test cert pem");
    let key = PrivateKeyDer::from_pem_slice(&cert.key_pem).expect("test key pem");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("tls 1.3")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server config");
    let conn = ServerConnection::new(Arc::new(config)).expect("server conn");
    let tls = StreamOwned::new(conn, stream);
    tungstenite::accept(tls).ok()
}
