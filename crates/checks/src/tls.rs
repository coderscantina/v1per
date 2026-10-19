//! Records the server certificate chain and its verdict during the handshake, so a check can
//! report certificate details even when `ignore_tls` lets an invalid chain through.

use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::{WebPkiServerVerifier, verify_server_name};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::ParsedCertificate;
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, SignatureScheme};
use x509_parser::extensions::GeneralName;
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::context::CheckContext;
use crate::result::{CertInfo, CertType, TlsInfo};

pub(crate) struct Capture {
    chain: Vec<CertificateDer<'static>>,
    verdict: Result<(), rustls::Error>,
}

/// Holds the first handshake of a check; later ones (redirects) are ignored like Kuma's
/// `once("keylog")`.
#[derive(Clone, Default)]
pub(crate) struct CaptureSlot(Arc<Mutex<Option<Capture>>>);

impl CaptureSlot {
    pub(crate) fn take(&self) -> Option<Capture> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

#[derive(Debug)]
struct RecordingVerifier {
    inner: Arc<WebPkiServerVerifier>,
    ignore_tls: bool,
    slot: CaptureSlot,
}

impl std::fmt::Debug for CaptureSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CaptureSlot")
    }
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verdict = self
            .inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            .map(|_| ());
        let mut slot = self.slot.0.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            let chain = std::iter::once(end_entity)
                .chain(intermediates)
                .map(|cert| cert.clone().into_owned())
                .collect();
            *slot = Some(Capture {
                chain,
                verdict: verdict.clone(),
            });
        }
        match verdict {
            Err(error) if !self.ignore_tls => Err(error),
            _ => Ok(ServerCertVerified::assertion()),
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
        self.inner.supported_verify_schemes()
    }
}

/// A fresh client config per check, so no TLS session is reused (Kuma sets
/// `maxCachedSessions: 0`).
pub(crate) fn client_config(
    ctx: &CheckContext,
    ignore_tls: bool,
    slot: &CaptureSlot,
) -> ClientConfig {
    ClientConfig::builder_with_provider(ctx.provider.clone())
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(RecordingVerifier {
            inner: ctx.verifier.clone(),
            ignore_tls,
            slot: slot.clone(),
        }))
        .with_no_client_auth()
}

impl Capture {
    pub(crate) fn info(&self, host: &str) -> TlsInfo {
        let name_error = matches!(
            self.verdict,
            Err(rustls::Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }
            ))
        );
        let hostname_match = match &self.verdict {
            Ok(()) => true,
            Err(_) if name_error => false,
            Err(_) => self.names(host),
        };
        let now = Utc::now();
        let parsed: Vec<_> = self
            .chain
            .iter()
            .filter_map(|der| {
                X509Certificate::from_der(der)
                    .ok()
                    .map(|(_, cert)| (der, cert))
            })
            .collect();
        let chain = parsed
            .iter()
            .enumerate()
            .map(|(index, (der, cert))| cert_info(der, cert, index, parsed.len(), now))
            .collect();
        TlsInfo {
            valid: self.verdict.is_ok() || name_error,
            hostname_match,
            chain,
        }
    }

    fn names(&self, host: &str) -> bool {
        let Some(leaf) = self.chain.first() else {
            return false;
        };
        let (Ok(name), Ok(cert)) = (
            ServerName::try_from(host),
            ParsedCertificate::try_from(leaf),
        ) else {
            return false;
        };
        verify_server_name(&cert, &name).is_ok()
    }

    /// The handshake error in Node's words, when verification rejected the chain.
    pub(crate) fn rejection(&self, host: &str, info: &TlsInfo) -> Option<String> {
        let Err(rustls::Error::InvalidCertificate(error)) = &self.verdict else {
            return self.verdict.as_ref().err().map(ToString::to_string);
        };
        Some(match error {
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => {
                "certificate has expired".to_owned()
            }
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                "certificate is not yet valid".to_owned()
            }
            CertificateError::UnknownIssuer => match info.chain.as_slice() {
                [leaf] if leaf.cert_type == CertType::SelfSigned => {
                    "self-signed certificate".to_owned()
                }
                chain if chain.iter().any(|cert| cert.cert_type == CertType::RootCa) => {
                    "self-signed certificate in certificate chain".to_owned()
                }
                _ => "unable to verify the first certificate".to_owned(),
            },
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                let names = info
                    .chain
                    .first()
                    .map(|leaf| {
                        leaf.valid_for
                            .iter()
                            .map(|name| match name.parse::<std::net::IpAddr>() {
                                Ok(_) => format!("IP Address:{name}"),
                                Err(_) => format!("DNS:{name}"),
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!(
                    "Hostname/IP does not match certificate's altnames: Host: {host}. is not in the cert's altnames: {names}"
                )
            }
            other => other.to_string(),
        })
    }
}

fn cert_info(
    der: &CertificateDer<'_>,
    cert: &X509Certificate<'_>,
    index: usize,
    chain_len: usize,
    now: DateTime<Utc>,
) -> CertInfo {
    let self_issued = cert.subject().as_raw() == cert.issuer().as_raw();
    let cert_type = match (index, self_issued) {
        (0, true) if chain_len == 1 => CertType::SelfSigned,
        (0, _) => CertType::Server,
        (_, true) => CertType::RootCa,
        (_, false) => CertType::IntermediateCa,
    };
    let time = |asn1: x509_parser::time::ASN1Time| {
        DateTime::from_timestamp(asn1.timestamp(), 0).unwrap_or_default()
    };
    let valid_to = time(cert.validity().not_after);
    let valid_for = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|san| {
            san.value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(dns) => Some((*dns).to_owned()),
                    GeneralName::IPAddress(bytes) => ip_from_bytes(bytes).map(|ip| ip.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let fingerprint = ring::digest::digest(&ring::digest::SHA256, der);
    CertInfo {
        subject: display_name(cert.subject()),
        issuer: display_name(cert.issuer()),
        valid_from: time(cert.validity().not_before),
        valid_to,
        days_remaining: (valid_to - now).num_days(),
        fingerprint256: fingerprint
            .as_ref()
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
        cert_type,
        valid_for,
    }
}

/// The common name, or the full distinguished name when there is none.
fn display_name(name: &x509_parser::x509::X509Name<'_>) -> String {
    name.iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_string())
}

fn ip_from_bytes(bytes: &[u8]) -> Option<std::net::IpAddr> {
    match bytes.len() {
        4 => Some(<[u8; 4]>::try_from(bytes).ok()?.into()),
        16 => Some(<[u8; 16]>::try_from(bytes).ok()?.into()),
        _ => None,
    }
}
