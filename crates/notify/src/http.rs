use std::error::Error as _;
use std::sync::Arc;

use reqwest::{RequestBuilder, Url};
use rustls::{ClientConfig, RootCertStore};

use crate::Delivered;
use crate::error::SendError;
use crate::secret::Secret;

/// Longest provider error body kept in a [`SendError`].
const MAX_ERROR_BODY: usize = 500;

pub(crate) fn client() -> reqwest::Client {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .user_agent(concat!("v1per/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("a client with a preconfigured rustls backend builds")
}

/// Parses a configured URL. The error never echoes it, since webhook URLs carry tokens.
pub(crate) fn url(raw: Option<&str>, field: &str) -> Result<Url, SendError> {
    let raw = raw.ok_or_else(|| SendError::Config(format!("{field} is missing")))?;
    Url::parse(raw).map_err(|_| SendError::Config(format!("{field} is not a valid URL")))
}

pub(crate) fn secret_url(raw: Option<&Secret>, field: &str) -> Result<Url, SendError> {
    url(raw.map(Secret::expose), field)
}

/// Sends the request; any 2xx is success, like axios.
pub(crate) async fn deliver(request: RequestBuilder) -> Result<Delivered, SendError> {
    let response = request.send().await.map_err(transport_error)?;
    let status = response.status();
    if status.is_success() {
        return Ok(Delivered {
            detail: format!("HTTP {}", status.as_u16()),
        });
    }
    let mut body = response.text().await.unwrap_or_default();
    if body.len() > MAX_ERROR_BODY {
        let cut = body.floor_char_boundary(MAX_ERROR_BODY);
        body.truncate(cut);
    }
    Err(SendError::Http {
        status: status.as_u16(),
        body,
    })
}

/// Connect, TLS and timeout failures, with the URL stripped.
pub(crate) fn transport_error(error: reqwest::Error) -> SendError {
    let error = error.without_url();
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    SendError::Connection(message)
}
