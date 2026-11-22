//! Test certificates and a blocking TLS client, for this crate's tests and its dependents' (feature `test-support`).
//! Never enabled in a shipped build.

use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    date_time_ymd,
};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use skyelab_tailnet::Cert;

/// The MagicDNS name the fake tailnets in tests give this node.
pub const NODE_NAME: &str = "desktop.example.ts.net";

/// A throwaway certificate authority.
pub struct TestCa {
    issuer: Issuer<'static, KeyPair>,
    root: CertificateDer<'static>,
    root_pem: String,
}

impl Default for TestCa {
    fn default() -> Self {
        Self::new()
    }
}

impl TestCa {
    pub fn new() -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("ca params");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        // A distinct name per CA, so a cert from another one is an unknown issuer, not a bad signature.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        params
            .distinguished_name
            .push(DnType::CommonName, format!("SkyeLab Test CA {n}"));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().expect("ca key");
        let root = params.self_signed(&key).expect("ca cert");
        Self {
            issuer: Issuer::new(params, key),
            root: root.der().clone(),
            root_pem: root.pem(),
        }
    }

    /// The root to trust.
    pub fn root(&self) -> CertificateDer<'static> {
        self.root.clone()
    }

    /// A leaf for `names`, valid from 2020 until `until` (year, month, day). PEM chain is leaf then CA.
    pub fn issue_until(&self, names: &[&str], until: (i32, u8, u8)) -> Cert {
        let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        let mut params = CertificateParams::new(names).expect("leaf params");
        params.not_before = date_time_ymd(2020, 1, 1);
        params.not_after = date_time_ymd(until.0, until.1, until.2);
        let key = KeyPair::generate().expect("leaf key");
        let leaf = params.signed_by(&key, &self.issuer).expect("sign leaf");
        Cert {
            cert_pem: format!("{}{}", leaf.pem(), self.root_pem).into_bytes(),
            key_pem: key.serialize_pem().into_bytes(),
        }
    }

    /// A valid leaf for `name` (until 2099).
    pub fn issue(&self, name: &str) -> Cert {
        self.issue_until(&[name], (2099, 1, 1))
    }

    /// A leaf for `name` that expired in 2021.
    pub fn issue_expired(&self, name: &str) -> Cert {
        self.issue_until(&[name], (2021, 1, 1))
    }
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// A blocking TLS connection to `addr` that trusts only `root` and checks the cert names `name`.
pub fn tls_connect(
    addr: SocketAddr,
    name: &str,
    root: CertificateDer<'static>,
) -> std::io::Result<TlsStream> {
    let mut roots = RootCertStore::empty();
    roots.add(root).expect("add test root");
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("tls 1.3")
        .with_root_certificates(roots)
        .with_no_client_auth();
    let server = ServerName::try_from(name.to_owned()).expect("test server name");
    let conn = ClientConnection::new(Arc::new(config), server).expect("client connection");
    let tcp = TcpStream::connect(addr)?;
    tcp.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut tls = StreamOwned::new(conn, tcp);
    // Finish the handshake here, so a refused certificate fails this call, not the first WebSocket read.
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(tls)
}
