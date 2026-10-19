use std::fmt;
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::CertificateDer;
use rustls::{ClientConfig, RootCertStore};

use crate::oauth::OAuthTokens;

/// Default cap on response bytes read per http check.
pub const DEFAULT_MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

/// Shared state for running checks: trusted roots, cached OAuth tokens and limits.
/// Build one per probe and share it across checks.
pub struct CheckContext {
    pub(crate) provider: Arc<CryptoProvider>,
    pub(crate) verifier: Arc<WebPkiServerVerifier>,
    /// Token requests verify TLS strictly, like Kuma's openid-client.
    pub(crate) oauth_client: reqwest::Client,
    pub(crate) oauth_tokens: OAuthTokens,
    pub(crate) max_body_bytes: usize,
}

#[derive(Debug)]
pub struct InvalidRootCert(String);

impl fmt::Display for InvalidRootCert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid root certificate: {}", self.0)
    }
}

impl std::error::Error for InvalidRootCert {}

impl CheckContext {
    /// Trusts Mozilla's root store, as Node does by default.
    pub fn new() -> Self {
        Self::with_extra_root_certs(Vec::<Vec<u8>>::new()).expect("the bundled root store is valid")
    }

    /// Trusts Mozilla's roots plus the given DER certificates, like `NODE_EXTRA_CA_CERTS`.
    pub fn with_extra_root_certs(
        certs: impl IntoIterator<Item = Vec<u8>>,
    ) -> Result<Self, InvalidRootCert> {
        let mut roots = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        for der in certs {
            roots
                .add(CertificateDer::from(der))
                .map_err(|error| InvalidRootCert(error.to_string()))?;
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .map_err(|error| InvalidRootCert(error.to_string()))?;
        let tls = ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .expect("ring supports the default protocol versions")
            .with_webpki_verifier(verifier.clone())
            .with_no_client_auth();
        let oauth_client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .build()
            .expect("a client with a preconfigured rustls backend builds");
        Ok(Self {
            provider,
            verifier,
            oauth_client,
            oauth_tokens: OAuthTokens::default(),
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
        })
    }

    /// Caps the response bytes an http check reads; keyword and json-query see only this much.
    pub fn with_max_body_bytes(mut self, max_body_bytes: usize) -> Self {
        self.max_body_bytes = max_body_bytes;
        self
    }
}

impl Default for CheckContext {
    fn default() -> Self {
        Self::new()
    }
}
