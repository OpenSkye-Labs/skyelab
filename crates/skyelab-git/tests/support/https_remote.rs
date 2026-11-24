//! A fake https git remote for tests and live runs (never shipped): bare repos under a folder, served over https on
//! 127.0.0.1 by `git http-backend` (git's smart HTTP), with a certificate from a CA made for this run. Every request
//! needs Basic auth with the fake gh's `x-access-token:fake-token`, like a private GitHub repo, so a push or fetch
//! that works got its credentials from the bound gh.
//!
//! Only the child git trusts the CA, through the file [`HttpsRemote::ca_file`] names (`skyelab_git::trust_test_ca`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// The largest request body served (a test push is a few KiB); a bigger one is refused, never allocated.
const MAX_BODY: usize = 64 << 20;

/// The credentials the fake gh's `auth git-credential` answers with.
pub const TOKEN_AUTH: &str = "x-access-token:fake-token";

/// One request the server answered: method, path and query, whether its credentials were right.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub authorized: bool,
}

pub struct HttpsRemote {
    /// `https://127.0.0.1:<port>`; a repo `demo.git` under the root is at `<base>/demo.git`.
    pub base: String,
    /// The CA's certificate (PEM), for `GIT_SSL_CAINFO`.
    pub ca_file: PathBuf,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    _dir: tempfile::TempDir,
}

impl HttpsRemote {
    /// Serves the bare repos under `root` until the process ends.
    pub fn start(root: &Path) -> Self {
        let dir = tempfile::tempdir().expect("ca dir");
        let (config, ca_pem) = tls_config();
        let ca_file = dir.path().join("fake-remote-ca.pem");
        std::fs::write(&ca_file, ca_pem).expect("write ca");
        let listener = skyelab_core::process::exclusive(|| TcpListener::bind("127.0.0.1:0"))
            .expect("bind 127.0.0.1");
        let port = listener.local_addr().expect("addr").port();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let (root, log) = (root.to_owned(), seen.clone());
        std::thread::spawn(move || {
            for tcp in listener.incoming().flatten() {
                let (config, root, log) = (config.clone(), root.clone(), log.clone());
                std::thread::spawn(move || {
                    let _ = serve(tcp, config, &root, &log);
                });
            }
        });
        Self {
            base: format!("https://127.0.0.1:{port}"),
            ca_file,
            seen,
            _dir: dir,
        }
    }
}

/// A CA made for this run and a leaf for 127.0.0.1 it signed; the server config and the CA's PEM.
fn tls_config() -> (Arc<ServerConfig>, String) {
    let mut ca = CertificateParams::new(Vec::<String>::new()).expect("ca params");
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.distinguished_name
        .push(DnType::CommonName, "SkyeLab fake remote CA");
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate().expect("ca key");
    let ca_cert = ca.self_signed(&ca_key).expect("ca cert");
    let ca_pem = ca_cert.pem();
    let issuer = Issuer::new(ca, ca_key);
    let leaf = CertificateParams::new(vec!["127.0.0.1".to_owned()]).expect("leaf params");
    let key = KeyPair::generate().expect("leaf key");
    let cert = leaf.signed_by(&key, &issuer).expect("sign leaf");
    let chain = vec![
        cert.der().clone(),
        CertificateDer::from(ca_cert.der().to_vec()),
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("tls versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server cert");
    (Arc::new(config), ca_pem)
}

/// One request on one connection, answered by `git http-backend`, then the connection closes.
fn serve(
    tcp: TcpStream,
    config: Arc<ServerConfig>,
    root: &Path,
    log: &Mutex<Vec<Seen>>,
) -> std::io::Result<()> {
    tcp.set_read_timeout(Some(Duration::from_secs(60)))?;
    let conn = ServerConnection::new(config).map_err(std::io::Error::other)?;
    let mut tls = BufReader::new(StreamOwned::new(conn, tcp));
    let mut line = String::new();
    tls.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or_default().to_owned(),
        parts.next().unwrap_or_default().to_owned(),
    );
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        let mut h = String::new();
        tls.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(TOKEN_AUTH)
    );
    let authorized = header("authorization") == Some(expected.as_str());
    log.lock().expect("seen lock").push(Seen {
        method: method.clone(),
        target: target.clone(),
        authorized,
    });
    if !authorized {
        let out = tls.get_mut();
        out.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"fake\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )?;
        out.flush()?;
        return Ok(());
    }
    if header("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
        tls.get_mut().write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        tls.get_mut().flush()?;
    }
    let body = if header("transfer-encoding").is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
        read_chunked(&mut tls)?
    } else {
        let n: usize = header("content-length")
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        if n > MAX_BODY {
            return Err(std::io::Error::other("request body too large"));
        }
        let mut body = vec![0; n];
        tls.read_exact(&mut body)?;
        body
    };
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let mut cmd = Command::new("git");
    cmd.arg("http-backend")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_PROJECT_ROOT", root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("REMOTE_USER", "x-access-token")
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("REQUEST_METHOD", &method)
        .env("PATH_INFO", path)
        .env("QUERY_STRING", query)
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("CONTENT_TYPE", header("content-type").unwrap_or_default())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(enc) = header("content-encoding") {
        cmd.env("HTTP_CONTENT_ENCODING", enc);
    }
    if let Some(proto) = header("git-protocol") {
        cmd.env("GIT_PROTOCOL", proto);
    }
    let mut child = skyelab_core::process::spawn(&mut cmd)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // A writer thread, so a backend that answers before reading all its input never deadlocks against us.
    let writer = std::thread::spawn(move || stdin.write_all(&body));
    let mut cgi = BufReader::new(child.stdout.take().expect("piped stdout"));
    let (mut status, mut out_headers) = ("200 OK".to_owned(), String::new());
    loop {
        let mut h = String::new();
        if cgi.read_line(&mut h)? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        match h.split_once(':') {
            Some((k, v)) if k.eq_ignore_ascii_case("status") => status = v.trim().to_owned(),
            _ => {
                out_headers.push_str(h);
                out_headers.push_str("\r\n");
            }
        }
    }
    let out = tls.get_mut();
    write!(
        out,
        "HTTP/1.1 {status}\r\n{out_headers}Connection: close\r\n\r\n"
    )?;
    std::io::copy(&mut cgi, out)?;
    out.conn.send_close_notify();
    out.flush()?;
    let _ = writer.join();
    let _ = child.wait();
    Ok(())
}

fn read_chunked(r: &mut impl BufRead) -> std::io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut size = String::new();
        r.read_line(&mut size)?;
        let hex = size.trim().split(';').next().unwrap_or_default();
        let n = usize::from_str_radix(hex, 16).map_err(std::io::Error::other)?;
        if n == 0 {
            let mut end = String::new();
            r.read_line(&mut end)?;
            return Ok(body);
        }
        let start = body.len();
        if start + n > MAX_BODY {
            return Err(std::io::Error::other("request body too large"));
        }
        body.resize(start + n, 0);
        r.read_exact(&mut body[start..])?;
        let mut crlf = [0u8; 2];
        r.read_exact(&mut crlf)?;
    }
}
