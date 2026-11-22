//! Server authentication. The chain must verify against the Mozilla roots (webpki-roots, so Let's Encrypt), and
//! the leaf must carry the dialled MagicDNS name exactly: a wildcard that would also match is not enough.

use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};

/// The roots a shipped client trusts.
pub(crate) fn webpki_roots() -> Arc<RootCertStore> {
    Arc::new(RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    })
}

/// TLS 1.3 only (both ends are ours), verified by [`ExactName`] over `roots`.
pub(crate) fn config(roots: Arc<RootCertStore>) -> std::io::Result<Arc<ClientConfig>> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let inner = WebPkiServerVerifier::builder_with_provider(roots, provider.clone())
        .build()
        .map_err(std::io::Error::other)?;
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(std::io::Error::other)?
        .dangerous() // only to add a check on top of the standard verifier, never to skip it
        .with_custom_certificate_verifier(Arc::new(ExactName { inner, provider }))
        .with_no_client_auth();
    Ok(Arc::new(config))
}

#[derive(Debug)]
struct ExactName {
    inner: Arc<WebPkiServerVerifier>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for ExactName {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        // Chain, expiry and the standard name match first; this only narrows it.
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let not_for_name = rustls::Error::InvalidCertificate(CertificateError::NotValidForName);
        let ServerName::DnsName(want) = server_name else {
            return Err(not_for_name);
        };
        let cert = webpki::EndEntityCert::try_from(end_entity)
            .map_err(|_| rustls::Error::InvalidCertificate(CertificateError::BadEncoding))?;
        // valid_dns_names is fine here: it only ever narrows what the standard check above accepted.
        if cert
            .valid_dns_names()
            .any(|n| n.eq_ignore_ascii_case(want.as_ref()))
        {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(not_for_name)
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}
