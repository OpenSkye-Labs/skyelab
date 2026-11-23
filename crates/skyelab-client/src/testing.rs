//! Client side of the shared remote-test harness (`skyelab_daemon::testing` has the daemon side). Tests only.
//!
//! It lives here, not in skyelab-daemon, because skyelab-client's own tests use the daemon harness: a daemon that
//! depended on skyelab-client would hand those tests a second copy of the crate, whose `Options` isn't this one.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::CertificateDer;

use crate::Options;

/// The device key [`options`] sends; the daemon harness (`skyelab_daemon::testing::config`) approves the same one.
pub const DEVICE_KEY: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

/// Options for an in-process daemon on loopback: dial the MagicDNS name at 127.0.0.1 (TLS still checks the name),
/// trust only `root` (the daemon harness's `ca().root()`), and retry quickly.
pub fn options(port: u16, root: CertificateDer<'static>) -> Options {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(root).expect("add test root");
    Options {
        port,
        allow_loopback: true,
        resolve_to: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        roots: Some(Arc::new(roots)),
        backoff_min: Duration::from_millis(50),
        backoff_max: Duration::from_millis(200),
        device_key: Some(DEVICE_KEY.into()),
        ..Options::default()
    }
}
