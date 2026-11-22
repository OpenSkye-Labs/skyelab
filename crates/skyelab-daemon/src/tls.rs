//! This node's TLS certificate: fetched with `tailscale cert`, checked, served, and renewed in place.

use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use skyelab_tailnet::Cert;
use tokio_rustls::TlsAcceptor;
use x509_parser::extensions::GeneralName;

/// Where the fix for every "no certificate" error lives.
pub const ADMIN_DNS_URL: &str = "https://login.tailscale.com/admin/dns";

/// The certificate the daemon is serving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertInfo {
    /// The MagicDNS name it is for; clients must dial exactly this.
    pub name: String,
    pub not_after: SystemTime,
}

impl CertInfo {
    /// `not_after` as `YYYY-MM-DD HH:MM UTC`.
    pub fn expires_utc(&self) -> String {
        let secs = self
            .not_after
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
        // Howard Hinnant's days-to-civil.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        format!(
            "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
            rem / 3600,
            rem % 3600 / 60
        )
    }
}

/// The MagicDNS name to get a certificate for, from `Self.DNSName`.
pub(crate) fn node_name(dns_name: &str) -> Result<String> {
    match dns_name.trim_end_matches('.') {
        "" => bail!(
            "MagicDNS is off for this tailnet, so this machine has no name to get a TLS certificate for. Turn on \
             MagicDNS and HTTPS Certificates in the Tailscale admin console ({ADMIN_DNS_URL})"
        ),
        name => Ok(name.to_ascii_lowercase()),
    }
}

/// Parses and checks a cert for `name`: the leaf names it exactly, is valid now, and matches the key.
fn load(
    cert: &Cert,
    name: &str,
    provider: &CryptoProvider,
) -> Result<(Arc<CertifiedKey>, CertInfo)> {
    let chain = CertificateDer::pem_slice_iter(&cert.cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .context("the certificate file isn't valid PEM")?;
    let Some(leaf) = chain.first() else {
        bail!("the certificate file holds no certificate");
    };
    let key = PrivateKeyDer::from_pem_slice(&cert.key_pem)
        .context("the key file isn't a valid PEM private key")?;
    let (_, parsed) =
        x509_parser::parse_x509_certificate(leaf).context("can't parse the certificate")?;
    let names: Vec<String> = parsed
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|san| {
            san.value
                .general_names
                .iter()
                .filter_map(|n| match n {
                    GeneralName::DNSName(d) => Some(d.to_ascii_lowercase()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if !names.iter().any(|n| n == name) {
        bail!("the certificate is for {names:?}, not {name}");
    }
    let validity = parsed.validity();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if now < validity.not_before.timestamp() {
        bail!("the certificate for {name} isn't valid yet");
    }
    let not_after = validity.not_after.timestamp();
    if now >= not_after {
        bail!("the certificate for {name} expired");
    }
    let signing = provider
        .key_provider
        .load_private_key(key)
        .context("unsupported private key")?;
    let certified = CertifiedKey::new(chain, signing);
    certified
        .keys_match()
        .context("the private key doesn't match the certificate")?;
    let info = CertInfo {
        name: name.to_owned(),
        not_after: UNIX_EPOCH + Duration::from_secs(not_after.max(0) as u64),
    };
    Ok((Arc::new(certified), info))
}

#[derive(Debug)]
struct Resolver(RwLock<Arc<CertifiedKey>>);

impl ResolvesServerCert for Resolver {
    fn resolve(&self, _: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(
            self.0
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        )
    }
}

/// The serving side: an acceptor whose certificate can be swapped while connections stay up.
pub(crate) struct Tls {
    pub(crate) acceptor: TlsAcceptor,
    resolver: Arc<Resolver>,
    provider: Arc<CryptoProvider>,
    info: Mutex<CertInfo>,
}

impl Tls {
    pub(crate) fn new(cert: &Cert, name: &str) -> Result<Self> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let (key, info) = load(cert, name, &provider)?;
        let resolver = Arc::new(Resolver(RwLock::new(key)));
        let config = rustls::ServerConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .context("TLS 1.3 with ring")?
            .with_no_client_auth()
            .with_cert_resolver(resolver.clone());
        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            resolver,
            provider,
            info: Mutex::new(info),
        })
    }

    pub(crate) fn info(&self) -> CertInfo {
        self.info
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Serves `cert` from now on, if it passes the same checks as the first one. Otherwise the old one stays.
    pub(crate) fn renew(&self, cert: &Cert) -> Result<()> {
        let name = self.info().name;
        let (key, info) = load(cert, &name, &self.provider)?;
        *self
            .resolver
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner) = key;
        *self.info.lock().unwrap_or_else(PoisonError::into_inner) = info;
        Ok(())
    }
}
