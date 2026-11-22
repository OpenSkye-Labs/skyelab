//! Live check on a real tailnet: serves this machine with its real `tailscale cert` and dials it by MagicDNS name.
//! `cargo run -p skyelab-client --example live -- <empty dir for the cert>`
//!
//! From the daemon's own machine the connection must be refused by `is_own_device` (this node itself), which runs
//! on the TCP peer before TLS; the daemon logs the reason on stderr.

use std::sync::Arc;
use std::time::Duration;

use skyelab_client::{Client, Options, Update};
use skyelab_daemon::{Config, Daemon, NotWired, StoreBackend};
use skyelab_store::Store;
use skyelab_tailnet::{Tailnet, TailscaleCli};

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: live <empty cert dir>"))?;
    let tailnet = Arc::new(TailscaleCli::default().with_cert_dir(&dir));
    let me = tailnet.self_node()?;
    println!("node {} {:?}", me.dns_name, me.tailscale_ips);
    let backend = Arc::new(StoreBackend::new(Store::open_in_memory()?, NotWired));
    let config = Config {
        port: 0,
        ..Config::default()
    };
    let daemon = Daemon::start(config, tailnet, backend)?;
    let cert = daemon.certificate();
    let port = daemon.local_addrs()[0].port();
    println!(
        "serving wss://{}:{port} on {:?}, cert valid until {}",
        cert.name,
        daemon.local_addrs(),
        cert.expires_utc()
    );
    let client = Client::connect(
        cert.name.clone(),
        Options {
            port,
            backoff_min: Duration::from_secs(5),
            ..Options::default()
        },
    )?;
    match client.updates().recv_timeout(Duration::from_secs(20)) {
        Ok(Update::Connected(h)) => println!("CONNECTED (unexpected from this machine): {h:?}"),
        Ok(other) => println!("client: {other:?}"),
        Err(e) => println!("client: no update: {e}"),
    }
    Ok(())
}
