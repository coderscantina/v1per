//! Heartbeats and monitor-level entries as they cross the durable inbox into DuckDB
//! (monitoring contract).

use serde::{Deserialize, Serialize};

/// Inbox payload encoding version. Ledger entries record it so a replay from another
/// encoding is refused.
pub const INBOX_FORMAT: i64 = 1;

/// A monitor's ID in `control.sqlite`. Plain integer row IDs: they compress to a few bits
/// per row in DuckDB, where 360 million heartbeats would make 16-byte UUIDs expensive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MonitorId(pub i64);

/// A probe's ID in `control.sqlite`, the built-in probe included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ProbeId(pub i64);

impl ProbeId {
    /// The pseudo probe `kuma-import` of imported Kuma heartbeats. Negative, so it never
    /// collides with a SQLite row ID.
    pub const KUMA_IMPORT: ProbeId = ProbeId(-1);
}

/// Kuma's four heartbeat statuses, with Kuma's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Status {
    Down = 0,
    Up = 1,
    Pending = 2,
    Maintenance = 3,
}

impl Status {
    pub(crate) fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Status::Down),
            1 => Some(Status::Up),
            2 => Some(Status::Pending),
            3 => Some(Status::Maintenance),
            _ => None,
        }
    }
}

/// One check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heartbeat {
    pub monitor: MonitorId,
    /// `None` for results the server decides without a probe: push, group, manual.
    pub probe: Option<ProbeId>,
    /// UTC, Unix milliseconds.
    pub time_ms: i64,
    pub status: Status,
    pub latency_ms: Option<u32>,
    /// Empty when the check gave none.
    pub message: String,
    pub retries: u32,
    /// http types only.
    pub http_status: Option<u16>,
    /// TLS certificate `notAfter`, Unix milliseconds; http types only.
    pub cert_expiry_ms: Option<i64>,
}

/// An agent's idempotent batch ID, unique per probe.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BatchId {
    pub probe: ProbeId,
    pub id: String,
}

/// The evaluated status of a monitor that runs on probes, recorded by the evaluator. It
/// counts in the monitor-level rollup series and writes no raw row. Monitors the server
/// decides itself (push, group, manual) send no entry: their heartbeat, with probe `None`,
/// already is the monitor-level entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorStatus {
    pub monitor: MonitorId,
    /// UTC, Unix milliseconds.
    pub time_ms: i64,
    pub status: Status,
    /// The representative latency the evaluator picked.
    pub latency_ms: Option<u32>,
}

/// One inbox row: raw heartbeats and monitor-level entries.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Payload {
    pub heartbeats: Vec<Heartbeat>,
    pub statuses: Vec<MonitorStatus>,
}

impl Payload {
    pub fn entries(&self) -> u64 {
        (self.heartbeats.len() + self.statuses.len()) as u64
    }
}

/// [`Payload`] by reference: serializes identically without copying.
#[derive(Serialize)]
struct PayloadRef<'a> {
    heartbeats: &'a [Heartbeat],
    statuses: &'a [MonitorStatus],
}

pub(crate) fn encode(heartbeats: &[Heartbeat], statuses: &[MonitorStatus]) -> Vec<u8> {
    postcard::to_stdvec(&PayloadRef {
        heartbeats,
        statuses,
    })
    .expect("entries always serialize")
}

pub(crate) fn decode(bytes: &[u8]) -> Result<Payload, postcard::Error> {
    postcard::from_bytes(bytes)
}
