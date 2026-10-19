//! The agent protocol under `/api/probe/*`, as `docs/contracts/probe-protocol.md` describes it.
//! Both sides live in this binary: the server's handlers and `v1per probe`.

use serde::{Deserialize, Serialize};
use v1per_checks::CheckDefinition;
use v1per_protocol::ids::MonitorId;
use v1per_protocol::operations::monitors::HeartbeatStatus;

use super::schedule::Next;

/// The protocol version this build speaks. A server refuses agents of any other.
pub const PROTOCOL: u32 = 1;
/// Request header naming the agent's protocol version.
pub const PROTOCOL_HEADER: &str = "x-v1per-probe-protocol";
/// Results one batch may carry.
pub const BATCH_RESULTS_MAX: usize = 1_000;
/// Longest agent batch ID, in characters.
pub const BATCH_ID_MAX_CHARS: usize = 128;

/// One monitor a probe checks. Credentials in `definition` arrive unsealed; they travel only in
/// answers to the probe's authenticated requests and are never written to the agent's disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Assignment {
    pub monitor: MonitorId,
    /// Results of another revision are dropped.
    pub revision: i64,
    pub interval_ms: i64,
    /// Between checks while this probe's status is pending.
    pub retry_interval_ms: i64,
    /// Failed checks in a row reported as pending before this probe reports down.
    pub max_retries: u32,
    /// A json-query mismatch is down at once (Kuma's `retry_only_on_status_code_failure`).
    pub down_on_query_mismatch: bool,
    /// When the monitor last changed, Unix milliseconds. Changed since the probe started
    /// means check at once.
    pub updated_at: i64,
    pub definition: CheckDefinition,
}

/// What a probe reports with every assignment poll.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    /// The agent's v1per version.
    pub version: String,
    pub checks_started: u64,
    pub checks_late: u64,
    pub buffered_results: u64,
    pub dropped_results: u64,
}

/// `POST /api/probe/assignments`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentsRequest {
    /// The version of the list the agent runs; absent at start.
    pub version: Option<String>,
    /// The agent's clock when it sent the request, Unix milliseconds.
    pub sent_at: i64,
    pub health: Health,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignmentList {
    pub version: String,
    /// Absent when the list still has the requested version.
    pub assignments: Option<Vec<Assignment>>,
}

/// One check's result after the probe's own retries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub monitor: MonitorId,
    pub revision: i64,
    /// When the check started, Unix milliseconds on the probe's clock.
    pub time: i64,
    /// `down`, `up` or `pending`.
    pub status: HeartbeatStatus,
    pub retries: u32,
    pub latency_ms: Option<u32>,
    pub message: String,
    pub http_status: Option<u16>,
    pub cert_expiry: Option<i64>,
    /// The certificate chain of an https check, for expiry alerts. Agents of milestone 06
    /// leave it out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsChain>,
}

/// The certificates a TLS server sent, server certificate first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsChain {
    /// The chain verifies against the probe's trusted roots.
    pub valid: bool,
    pub certificates: Vec<ChainCertificate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainCertificate {
    /// The common name, or the whole subject without one.
    pub subject: String,
    pub cert_type: v1per_checks::CertType,
    /// Unix milliseconds.
    pub valid_to: i64,
    /// SHA-256, colon-separated hex.
    pub fingerprint: String,
}

impl ProbeResult {
    /// A pending probe checks again after the retry interval.
    pub fn next(&self) -> Next {
        match self.status {
            HeartbeatStatus::Pending => Next::Retry,
            _ => Next::Interval,
        }
    }
}

/// `POST /api/probe/results`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultBatch {
    /// Unique per probe and stable across resends.
    pub batch: String,
    pub sent_at: i64,
    pub results: Vec<ProbeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchAck {
    pub batch: String,
    /// The batch was admitted before; nothing new was stored.
    pub repeated: bool,
}

/// Every refusal: a stable `error` code and a message for the agent's operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Refusal {
    pub error: String,
    pub message: String,
}
