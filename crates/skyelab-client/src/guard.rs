//! Which server addresses the client may dial.

use std::net::{IpAddr, SocketAddr};

/// True for Tailscale's address ranges: 100.64.0.0/10 and fd7a:115c:a1e0::/48 (IPv4-mapped forms included).
///
/// 100.64.0.0/10 is also carrier-grade NAT space, so with the VPN off a hostile DNS on such a carrier could still
/// point at another subscriber. Only TLS with the node's certificate closes that (docs/mobile.md).
pub fn is_tailnet_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            a == 100 && b & 0xc0 == 64
        }
        IpAddr::V6(v6) => matches!(v6.segments(), [0xfd7a, 0x115c, 0xa1e0, ..]),
    }
}

/// Keeps the addresses the client may dial. The caller dials exactly these, never the name again, so a second
/// lookup can't swap in another address.
pub(crate) fn allowed(
    host: &str,
    resolved: impl IntoIterator<Item = SocketAddr>,
    allow_loopback: bool,
) -> Result<Vec<SocketAddr>, String> {
    let (ok, refused): (Vec<_>, Vec<_>) = resolved.into_iter().partition(|a| {
        is_tailnet_ip(a.ip()) || (allow_loopback && a.ip().to_canonical().is_loopback())
    });
    if ok.is_empty() {
        let refused: Vec<_> = refused.iter().map(|a| a.ip().to_string()).collect();
        return Err(format!(
            "refusing to connect: {host} resolved to {} outside the tailnet (is Tailscale on?)",
            if refused.is_empty() {
                "nothing".to_owned()
            } else {
                refused.join(", ")
            }
        ));
    }
    Ok(ok)
}

pub(crate) async fn resolve(
    host: &str,
    port: u16,
    allow_loopback: bool,
) -> Result<Vec<SocketAddr>, String> {
    let resolved = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("can't resolve {host}: {e}"))?;
    allowed(host, resolved, allow_loopback)
}
