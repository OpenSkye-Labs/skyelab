//! The update checks' requests: a plain HTTPS GET of a public JSON document (GitHub's latest release, or an
//! agent CLI's npm `latest` record), and a release asset's download (streamed, redirects kept on GitHub).
//! No auth, no cookies, nothing but the request line, `Host`, `User-Agent` and `Accept`. Tokio and rustls are already
//! here for the remote client, so no HTTP client is added for these GETs.

use skyelab_core::update::Asset;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

/// A release body is a few KB; anything past this is refused.
pub const MAX_BODY: usize = 1 << 20;
/// An HTTP answer's head (status line and headers) is refused past this.
const HEAD_MAX: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);

/// The latest release: its tag (`v0.3.0`) and page (`html_url`), both untrusted until the caller checks them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    /// Its downloads (`assets[]`), for installing it.
    pub assets: Vec<Asset>,
}

/// GETs `url` (`https://host/path` only) and parses GitHub's release JSON. Blocks; call it off the UI thread.
pub fn latest_release(url: &str) -> io::Result<Release> {
    parse_release(&get(url, "application/vnd.github+json")?)
}

/// GETs an npm registry `.../<package>/latest` record (`url`, `https://` only) and returns its `version`. Blocks.
pub fn npm_latest(url: &str) -> io::Result<String> {
    parse_npm_latest(&get(url, "application/json")?)
}

/// `version` from an npm registry version record: at most 64 letters, digits, `.`, `-` and `+` (untrusted until
/// compared).
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
    let mut body = Vec::new();
    let got = request(
        url,
        accept,
        MAX_BODY as u64,
        &mut body,
        &mut |_, _| {},
        Some(TIMEOUT),
    )?;
    match got {
        Got::Body => Ok(body),
        Got::Redirect(_) => Err(io::Error::other("the update check was redirected")),
    }
}

/// The largest download accepted (an installer is around 100 MB).
pub const MAX_DOWNLOAD: u64 = 1 << 30;
/// A download fails when no byte arrives for this long.
const IDLE: Duration = Duration::from_secs(30);

/// GETs a release asset (`https://` only) into `sink`, reporting (bytes so far, total if known). GitHub answers with a
/// redirect to its file host; up to five are followed, and only to its own download hosts.
/// Blocks; call it off the UI thread.
pub fn download(
    url: &str,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> io::Result<()> {
    let mut url = url.to_owned();
    for _ in 0..=5 {
        match request(
            &url,
            "application/octet-stream",
            MAX_DOWNLOAD,
            sink,
            progress,
            None,
        )? {
            Got::Body => return Ok(()),
            Got::Redirect(next) if redirect_ok(&next) => url = next,
            Got::Redirect(_) => return Err(invalid("the download was redirected off GitHub")),
        }
    }
    Err(invalid("too many redirects"))
}

/// Whether a download may follow a redirect to `url`: https on GitHub's own hosts only.
pub(crate) fn redirect_ok(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = &rest[..rest.find('/').unwrap_or(rest.len())];
    matches!(
        host,
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    )
}

/// What one request got: a body (written to the sink) or a redirect.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Got {
    Body,
    Redirect(String),
}

/// One GET of `url` over TLS; the whole exchange is bounded by `limit` when given, each read by [`IDLE`].
fn request(
    url: &str,
    accept: &str,
    cap: u64,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
    limit: Option<Duration>,
) -> io::Result<Got> {
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
    let timed_out = || io::Error::new(io::ErrorKind::TimedOut, "the update request timed out");
    runtime.block_on(async {
        let exchange = async {
            let tls = tokio::time::timeout(TIMEOUT, async {
                let tcp = tokio::net::TcpStream::connect((host, 443)).await?;
                connector.connect(name, tcp).await
            })
            .await
            .map_err(|_| timed_out())??;
            exchange(tls, host, path, accept, cap, sink, progress).await
        };
        match limit {
            Some(limit) => tokio::time::timeout(limit, exchange)
                .await
                .map_err(|_| timed_out())?,
            None => exchange.await,
        }
    })
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// One HTTP/1.0 GET over `stream` (1.0, so the body is never chunked and ends at EOF). A 200's body streams into
/// `sink`, at most `cap` bytes and all of a `Content-Length`; a 3xx returns its `Location`; anything else fails.
pub(crate) async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    host: &str,
    path: &str,
    accept: &str,
    cap: u64,
    sink: &mut dyn Write,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> io::Result<Got> {
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: SkyeLab/{}\r\nAccept: {accept}\r\n\r\n",
        env!("CARGO_PKG_VERSION")
    );
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;
    let mut buf = vec![0u8; 64 * 1024];
    let read = async |stream: &mut S, buf: &mut [u8]| {
        tokio::time::timeout(IDLE, stream.read(buf))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the download stalled"))?
    };
    // The head, bounded.
    let mut raw = Vec::new();
    let split = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            if i > HEAD_MAX {
                return Err(invalid("the HTTP header is too large"));
            }
            break i;
        }
        if raw.len() > HEAD_MAX {
            return Err(invalid("the HTTP header is too large"));
        }
        let n = read(&mut stream, &mut buf).await?;
        if n == 0 {
            return Err(invalid("no HTTP header end"));
        }
        raw.extend_from_slice(&buf[..n]);
    };
    let head = std::str::from_utf8(&raw[..split]).map_err(|_| invalid("bad HTTP header"))?;
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .ok_or_else(|| invalid("no HTTP status"))?;
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_owned())
        })
    };
    if matches!(status, "301" | "302" | "303" | "307" | "308") {
        let to = header("location").ok_or_else(|| invalid("a redirect with no Location"))?;
        return Ok(Got::Redirect(to));
    }
    if status != "200" {
        // GitHub's latest release is a 404 while the repo has published none.
        let kind = if status == "404" {
            io::ErrorKind::NotFound
        } else {
            io::ErrorKind::Other
        };
        return Err(io::Error::new(kind, format!("{host} answered {status}")));
    }
    let total = header("content-length").and_then(|v| v.parse::<u64>().ok());
    if total.is_some_and(|t| t > cap) {
        return Err(invalid("the answer is too large"));
    }
    let mut done = 0u64;
    let mut chunk = raw.split_off(split + 4);
    loop {
        // Bytes past a stated length aren't part of the body.
        if let Some(t) = total {
            chunk.truncate(usize::try_from(t - done).unwrap_or(usize::MAX));
        }
        done += chunk.len() as u64;
        if done > cap {
            return Err(invalid("the answer is too large"));
        }
        sink.write_all(&chunk)?;
        progress(done, total);
        // All of it is in: don't wait on a server that keeps the connection open.
        if total == Some(done) {
            break;
        }
        let n = read(&mut stream, &mut buf).await?;
        if n == 0 {
            break;
        }
        chunk.clear();
        chunk.extend_from_slice(&buf[..n]);
    }
    if total.is_some_and(|t| t != done) {
        return Err(invalid("the answer was cut short"));
    }
    Ok(Got::Body)
}

/// `tag_name`, `html_url` and the assets' `name` / `browser_download_url` from GitHub's release JSON.
pub(crate) fn parse_release(body: &[u8]) -> io::Result<Release> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(io::Error::other)?;
    let field = |k: &str| {
        v.get(k)
            .and_then(|s| s.as_str())
            .filter(|s| !s.is_empty() && s.len() <= 512)
            .map(str::to_owned)
            .ok_or_else(|| invalid("the release has no tag or page"))
    };
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .into_iter()
        .flatten()
        .take(64)
        .filter_map(|a| {
            let s = |k: &str| {
                a.get(k)?
                    .as_str()
                    .filter(|s| s.len() <= 512)
                    .map(str::to_owned)
            };
            Some(Asset {
                name: s("name")?,
                url: s("browser_download_url")?,
            })
        })
        .collect();
    Ok(Release {
        tag: field("tag_name")?,
        url: field("html_url")?,
        assets,
    })
}
