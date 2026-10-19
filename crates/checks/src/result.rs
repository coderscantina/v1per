use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The verdict of one check run, after upside-down inversion. Retries and pending are the
/// evaluator's job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    pub status: Status,
    /// Set when the target answered, as Kuma sets `ping`.
    pub latency_ms: Option<u64>,
    /// Kuma-like heartbeat message.
    pub message: String,
    pub http_status: Option<u16>,
    /// Certificate details of the first TLS connection, for https URLs.
    pub tls: Option<TlsInfo>,
    /// Why the target failed, before inversion. `None` when it passed.
    pub failure: Option<Failure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Failure {
    /// The target did not pass the check.
    Check,
    /// The check hit its timeout.
    Timeout,
    /// A json-query comparison came out false. Kuma's `retry_only_on_status_code_failure`
    /// skips retries for this failure only.
    JsonQueryMismatch,
    /// The definition uses an option this crate does not run. Never inverted.
    Unsupported { option: Unsupported },
    /// The host refuses the probe the socket it needs (ICMP). Never inverted.
    NotPermitted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    NtlmAuth,
    MtlsAuth,
    /// The JSONata expression does not parse.
    JsonQuery {
        reason: String,
    },
}

/// Kuma's `monitor_tls_info`: chain validity, hostname match and parsed certificates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TlsInfo {
    /// The chain verifies against the trusted roots at check time, ignoring the hostname
    /// (Node's `socket.authorized`).
    pub valid: bool,
    /// The leaf certificate names the URL's host (Kuma's `hostnameMatchMonitorUrl`).
    pub hostname_match: bool,
    /// Leaf first, as the server sent it.
    pub chain: Vec<CertInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CertInfo {
    pub subject: String,
    pub issuer: String,
    pub valid_from: DateTime<Utc>,
    pub valid_to: DateTime<Utc>,
    /// Whole days until `valid_to`, negative once expired, truncated like Kuma's dayjs diff.
    pub days_remaining: i64,
    /// Colon-separated uppercase hex, as Node formats `fingerprint256`.
    pub fingerprint256: String,
    pub cert_type: CertType,
    /// Subject alternative names.
    pub valid_for: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CertType {
    Server,
    SelfSigned,
    IntermediateCa,
    RootCa,
}

impl CheckResult {
    pub(crate) fn up(message: impl Into<String>) -> Self {
        Self {
            status: Status::Up,
            latency_ms: None,
            message: message.into(),
            http_status: None,
            tls: None,
            failure: None,
        }
    }

    pub(crate) fn down(failure: Failure, message: impl Into<String>) -> Self {
        Self {
            status: Status::Down,
            latency_ms: None,
            message: message.into(),
            http_status: None,
            tls: None,
            failure: Some(failure),
        }
    }

    pub(crate) fn unsupported(option: Unsupported) -> Self {
        let message = match &option {
            Unsupported::NtlmAuth => "NTLM authentication is not supported".to_owned(),
            Unsupported::MtlsAuth => "mTLS authentication is not supported".to_owned(),
            Unsupported::JsonQuery { reason } => format!("JSON query is not supported: {reason}"),
        };
        Self::down(Failure::Unsupported { option }, message)
    }

    pub(crate) fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }

    /// Applies upside-down mode the way Kuma does before retries.
    pub(crate) fn invert(mut self) -> Self {
        match (self.status, &self.failure) {
            (_, Some(Failure::Unsupported { .. } | Failure::NotPermitted)) => {}
            (Status::Up, _) => {
                self.status = Status::Down;
                self.message = "Flip UP to DOWN".to_owned();
            }
            (Status::Down, _) => self.status = Status::Up,
        }
        self
    }
}
