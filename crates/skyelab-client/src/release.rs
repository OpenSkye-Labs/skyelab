//! The update checks' one request: a plain HTTPS GET of a public JSON document (GitHub's latest release, or an
//! agent CLI's npm `latest` record).
//! No auth, no cookies, nothing but the request line, `Host`, `User-Agent` and `Accept`. Tokio and rustls are already
//! here for the remote client, so no HTTP client is added for one GET.

use std::io;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

/// A release body is a few KB; anything past this is refused.
pub const MAX_BODY: usize = 1 << 20;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The latest release: its tag (`v0.3.0`) and page (`html_url`), both untrusted until the caller checks them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub url: String,
}

/// GETs `url` (`https://host/path` only) and parses GitHub's release JSON. Blocks; call it off the UI thread.
pub fn latest_release(url: &str) -> io::Result<Release> {
    parse_release(&get(url, "application/vnd.github+json")?)
}

/// GETs an npm registry `.../<package>/latest` record (`url`, `https://` only) and returns its `version`. Blocks.
pub fn npm_latest(url: &str) -> io::Result<String> {
    parse_npm_latest(&get(url, "application/json")?)
}

/// `version` from an npm registry version record: short, digits and dots first (untrusted until compared).
pub(crate) fn parse_npm_latest(body: &[u8]) -> io::Result<String> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(io::Error::other)?;
    v.get("version")
        .and_then(|s| s.as_str())
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 64
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || ".-+".contains(c))
        })
        .map(str::to_owned)
        .ok_or_else(|| invalid("the npm record has no version"))
}

/// One bounded GET of `url` over TLS, returning a 200's body.
fn get(url: &str, accept: &str) -> io::Result<Vec<u8>> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| invalid("not an https URL"))?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let name = rustls::pki_types::ServerName::try_from(host.to_owned())
        .map_err(|_| invalid("bad host name"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .with_root_certificates(crate::tls::webpki_roots())
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        tokio::time::timeout(TIMEOUT, async {
            let tcp = tokio::net::TcpStream::connect((host, 443)).await?;
            let tls = connector.connect(name, tcp).await?;
            get_over(tls, host, path, accept).await
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the update check timed out"))?
    })
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// One HTTP/1.0 GET over `stream` (1.0, so the body is never chunked and ends at EOF), returning a 200's body.
pub(crate) async fn get_over<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    host: &str,
    path: &str,
    accept: &str,
) -> io::Result<Vec<u8>> {
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: SkyeLab/{}\r\nAccept: {accept}\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;
    let mut raw = Vec::new();
    // Head plus body, bounded: one byte past the cap is enough to refuse.
    let cap = (MAX_BODY + 16 * 1024) as u64;
    (&mut stream).take(cap + 1).read_to_end(&mut raw).await?;
    if raw.len() as u64 > cap {
        return Err(invalid("the release answer is too large"));
    }
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| invalid("no HTTP header end"))?;
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| invalid("bad HTTP header"))?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or_else(|| invalid("no HTTP status"))?;
    if status != "200" {
        return Err(io::Error::other(format!("{host} answered {status}")));
    }
    let body = raw.split_off(split + 4);
    if body.len() > MAX_BODY {
        return Err(invalid("the release answer is too large"));
    }
    Ok(body)
}

/// `tag_name` and `html_url` from GitHub's release JSON.
pub(crate) fn parse_release(body: &[u8]) -> io::Result<Release> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(io::Error::other)?;
    let field = |k: &str| {
        v.get(k)
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .map(str::to_owned)
            .ok_or_else(|| invalid("the release has no tag or page"))
    };
    Ok(Release {
        tag: field("tag_name")?,
        url: field("html_url")?,
    })
}
