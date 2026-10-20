//! What this V1Per instance is, and its settings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::operations::channels::ChannelTestQueued;
use crate::operations::imports::ImportSummary;
use crate::operations::monitors::SecretField;
use crate::operations::probes::Probe;

crate::domain! {
    pub trait InstanceHandlers;
    commands {
        /// Changes the instance settings. Instance admins only (`forbidden`). A setting the
        /// server configuration fixes (`name_fixed`, `public_url_fixed`) is ignored: its stored
        /// value stays as it is.
        "instance.updateSettings" => instance_update_settings (Person): InstanceUpdateSettingsArgs => InstanceSettings;
        /// Sets or, without `smtp`, removes the instance SMTP server. Email channels without
        /// their own server send through it. A password without `value` keeps the stored one.
        /// Instance admins only (`forbidden`).
        "instance.updateSmtp" => instance_update_smtp (Person): InstanceUpdateSmtpArgs => InstanceSettings;
        /// Queues a test email to `to` through `smtp` as given, or through the saved server
        /// without it (`required` on `smtp` when none is saved). A password without `value`
        /// uses the saved one. Follow `channel.delivery` on the root node's `channels:<id>`
        /// topic and read the outcome with `channels.delivery`. Five a minute per person
        /// (`rate_limited`). Instance admins only (`forbidden`).
        "instance.testSmtp" => instance_test_smtp (Person): InstanceTestSmtpArgs => ChannelTestQueued;
    }
    queries {
        /// Name, version and public URL of the instance.
        "instance.info" => instance_info (Person): Empty => InstanceInfo;
        /// The instance settings. Instance admins only (`forbidden`).
        "instance.settings" => instance_settings (Person): Empty => InstanceSettings;
        /// Storage, probe, notification, retention and import health, as `v1per diagnostics`
        /// reports it. No event marks a change: read it again to refresh. Instance admins only
        /// (`forbidden`).
        "instance.operations" => instance_operations (Person): Empty => InstanceOperations;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceInfo {
    /// The operator's name for this instance.
    pub name: String,
    /// Server version.
    pub version: String,
    /// The public URL's origin, such as `https://status.example.com`. Absent when none is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceSettings {
    /// The name in effect.
    pub name: String,
    /// `V1PER_INSTANCE_NAME` sets the name; the stored one has no effect.
    pub name_fixed: bool,
    /// The public URL in effect, the base of invitation links.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_url: Option<String>,
    /// `V1PER_PUBLIC_URL` sets the public URL and also guards commands; the stored one has no
    /// effect.
    pub public_url_fixed: bool,
    /// The IANA zone of times in notifications and of maintenance without its own zone, like
    /// Kuma's server timezone. Default `UTC`.
    pub timezone: String,
    /// Days before a certificate expires at which monitors with `expiry_notification` alert,
    /// like Kuma's `tlsExpiryNotifyDays`. Default 7, 14 and 21.
    pub tls_expiry_notify_days: Vec<u32>,
    /// Absent until an admin sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub smtp: Option<SmtpSettings>,
}

/// The instance's outgoing mail server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SmtpSettings {
    pub host: String,
    pub port: u16,
    /// Implicit TLS. Off means STARTTLS when the server offers it.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub secure: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub ignore_tls_error: bool,
    /// Only with `secure` off: never upgrade to TLS.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub ignore_starttls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: SecretField,
    /// The sender address, such as `v1per <alerts@example.com>`.
    pub from: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceUpdateSmtpArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub smtp: Option<SmtpSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceTestSmtpArgs {
    /// The recipient, `name@example.com`.
    pub to: String,
    /// The form as it stands; absent tests the saved server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub smtp: Option<SmtpSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceUpdateSettingsArgs {
    /// 1 to 100 characters after trimming. Required unless `name_fixed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// An `http` or `https` URL; only its origin is kept. Absent clears it unless
    /// `public_url_fixed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub public_url: Option<String>,
    /// An IANA zone. Absent keeps the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
    /// 1 to 10 values from 1 to 365. Absent keeps the current ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tls_expiry_notify_days: Option<Vec<u32>>,
}

/// Operational state of the instance (milestone 11).
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InstanceOperations {
    /// Server version.
    pub version: String,
    pub generated_at: i64,
    pub schema: SchemaVersions,
    pub storage: StorageUsage,
    pub inbox: InboxHealth,
    pub heartbeats: HeartbeatHealth,
    /// Every probe on the instance, the built-in one first.
    pub probes: Vec<Probe>,
    pub outbox: OutboxHealth,
    pub retention: RetentionState,
    /// Backups, retention runs and diagnostics, newest first, at most 20.
    pub jobs: Vec<MaintenanceJob>,
    pub imports: Vec<ImportSummary>,
}

/// Schema version of each engine file: the latest migration of the SQLite files, the
/// heartbeat schema of DuckDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SchemaVersions {
    pub control: i64,
    pub inbox: i64,
    pub heartbeats: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct StorageUsage {
    /// The engine files and their write-ahead logs that exist, largest first.
    pub files: Vec<DataFile>,
    /// Everything in the data directory, spill files included.
    pub data_dir_bytes: u64,
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
    /// Below this much free space new results are refused.
    pub disk_reserve_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct DataFile {
    /// Name inside the data directory, such as `heartbeats.duckdb`.
    pub name: String,
    pub bytes: u64,
}

/// Results admitted and waiting to move into DuckDB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct InboxHealth {
    /// Admissions waiting.
    pub rows: u64,
    /// Heartbeats and monitor statuses in them.
    pub entries: u64,
    pub bytes: u64,
    /// How long the oldest has waited; 0 when the inbox is empty.
    pub oldest_age_ms: u64,
}

/// How far DuckDB has caught up with the inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeartbeatHealth {
    /// The last transfer batch applied and the last inbox sequence it holds.
    pub watermark_batch: i64,
    pub watermark_sequence: i64,
    /// Failed transfer attempts since the server started.
    pub transfer_failures: u64,
    /// Every transfer attempt failed since then; absent while transfer works.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub transfer_failing_since: Option<i64>,
}

/// The notification outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct OutboxHealth {
    /// Deliveries waiting to be sent, retries included.
    pub pending: u64,
    /// Pending deliveries that failed at least once.
    pub retrying: u64,
    /// When the oldest pending delivery was queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub oldest_pending_at: Option<i64>,
    /// Deliveries that gave up in the last 24 hours.
    pub failed_last_day: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionState {
    /// Days raw heartbeats are kept.
    pub raw_days: u32,
    /// Days minute rollups are kept; absent: forever. Hour and day rollups are always kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub minute_rollup_days: Option<u32>,
    /// The last retention run that finished, dry runs left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_run: Option<RetentionRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionRun {
    pub finished_at: i64,
    pub removed: RetentionCounts,
}

/// Rows a retention run removed, or a dry run would remove.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct RetentionCounts {
    /// Raw heartbeats past their retention.
    pub heartbeats: u64,
    pub minute_rollups: u64,
    /// Probe batch IDs kept to recognize resent batches, after 24 hours.
    pub agent_batches: u64,
    /// Finished notification deliveries, after 30 days.
    pub deliveries: u64,
    /// Expired or idle sign-in sessions.
    pub sessions: u64,
    pub login_challenges: u64,
    pub page_sessions: u64,
    pub page_email_codes: u64,
    /// Command answers past their replay window.
    pub idempotency_keys: u64,
    /// Finished maintenance jobs, after 30 days.
    pub jobs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceJob {
    pub kind: MaintenanceJobKind,
    pub state: MaintenanceJobState,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finished_at: Option<i64>,
    /// The outcome code of a failed job, such as `disk_full` or `interrupted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceJobKind {
    Retention,
    RetentionDryRun,
    Backup,
    Diagnostics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceJobState {
    Queued,
    Running,
    Done,
    Failed,
}
