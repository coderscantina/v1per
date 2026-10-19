//! Kuma imports (decision 0007): `v1per import kuma` writes them, instance admins read them.
//! One import per node: its progress, the reconciliation manifest and the inventory of what
//! was not mapped. Inventoried source values stay sealed; answers name their columns only.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::envelope::Empty;
use crate::ids::{MonitorId, NodeId};

crate::domain! {
    pub trait ImportsHandlers;
    commands {}
    queries {
        /// Every Kuma import on the instance, newest first. Instance admins only
        /// (`forbidden`).
        "imports.list" => imports_list (Person): Empty => ImportList;
        /// The Kuma import into `node`: its state, step progress and, once finished, the
        /// manifest. Instance admins only (`forbidden`); `not_found` when the node has none.
        "imports.get" => imports_get (Person): ImportArgs => ImportRun;
        /// What the import into `node` did not map, or mapped with a loss, in source order.
        /// Instance admins only (`forbidden`); `not_found` when the node has no import.
        "imports.inventory" => imports_inventory (Person): ImportInventoryArgs => ImportInventoryPage;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportArgs {
    pub node: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportInventoryArgs {
    pub node: NodeId,
    /// `next_cursor` of the page before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cursor: Option<String>,
    /// 1 to 500, default 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportList {
    pub imports: Vec<ImportSummary>,
}

/// One import, without its steps and manifest; `imports.get` has those.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportSummary {
    pub node: NodeId,
    pub node_name: String,
    /// The Kuma database as named on the command line.
    pub source: String,
    pub state: ImportState,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finished_at: Option<i64>,
    /// The manifest's verdict, once the import is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reconciled: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportState {
    /// In progress, or interrupted: `v1per import kuma` with the same source resumes it.
    Running,
    Complete,
}

/// Steps in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportStep {
    /// Nodes, monitors, channels, maintenance, pages, incidents, users and the inventory, in
    /// one transaction.
    Control,
    /// Kuma's `stat_*` rollups.
    Stats,
    /// Raw heartbeats inside the retention window.
    Heartbeats,
    /// Counts and uptime compared with the source; writes the manifest.
    Reconcile,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportRun {
    pub node: NodeId,
    /// The Kuma database as named on the command line.
    pub source: String,
    pub state: ImportState,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finished_at: Option<i64>,
    pub steps: Vec<ImportStepProgress>,
    /// Present once the import is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub manifest: Option<ImportManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportStepProgress {
    pub step: ImportStep,
    pub chunks: i64,
    /// Source rows written.
    pub rows: i64,
    /// Source rows left out, such as history of a deleted monitor.
    pub skipped: i64,
    pub done: bool,
}

/// What an import created, compared with its source. Written to the data directory as well.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportManifest {
    /// Every table, history count and uptime window matches.
    pub reconciled: bool,
    pub node: NodeId,
    pub source: ImportSource,
    /// The run's pinned start.
    pub started_at: i64,
    pub finished_at: i64,
    /// Raw heartbeats at or after this instant were imported.
    pub raw_since: i64,
    pub nodes: CountCheck,
    pub tables: Vec<TableCheck>,
    pub history: Vec<HistoryCheck>,
    pub uptime: UptimeCheck,
    /// Kuma users with the login they sign in with now.
    pub logins: Vec<ImportedLogin>,
    /// Losses that have no inventory row, such as tag colors.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportSource {
    pub path: String,
    /// Of the database file followed by its WAL, when one was read.
    pub sha256: String,
    pub bytes: u64,
    /// The source hashed the same before and after the import.
    pub unchanged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CountCheck {
    pub expected: u64,
    pub created: u64,
}

/// One Kuma configuration or inventory table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TableCheck {
    pub table: String,
    /// Rows in the source.
    pub source: u64,
    /// Rows that became v1per records.
    pub mapped: u64,
    /// Inventory entries for the table. A paused monitor is mapped and inventoried.
    pub inventoried: u64,
    /// v1per rows of the counterpart, where there is one table to count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub destination: Option<u64>,
    pub reconciled: bool,
}

/// One history table: `heartbeat` or a `stat_*` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct HistoryCheck {
    pub table: String,
    pub source: u64,
    /// Rows in heartbeat storage.
    pub imported: u64,
    /// Rows of monitors that no longer exist in the source.
    pub skipped: u64,
    /// Heartbeats before the raw retention window; only their rollups are imported.
    pub outside_window: u64,
    pub reconciled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct UptimeCheck {
    /// The windows end with the bucket holding this instant: the newest one Kuma wrote.
    pub as_of: i64,
    pub monitors: u64,
    /// Monitors whose 24 hour, 30 day and 1 year counts equal Kuma's.
    pub matched: u64,
    pub per_monitor: Vec<MonitorUptimeCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorUptimeCheck {
    pub monitor: MonitorId,
    pub kuma_id: i64,
    pub name: String,
    pub matched: bool,
    pub windows: Vec<WindowCheck>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub enum UptimeWindowName {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "30d")]
    Month,
    #[serde(rename = "1y")]
    Year,
}

/// Up and down beats in one window, pending counted as down and maintenance left out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct WindowCheck {
    pub window: UptimeWindowName,
    pub kuma: UptimeCounts,
    pub v1per: UptimeCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct UptimeCounts {
    pub up: u64,
    pub down: u64,
    /// From 0 to 1; absent without up or down beats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportedLogin {
    pub kuma_username: String,
    pub email: String,
    pub instance_admin: bool,
    /// The authenticator keeps working.
    pub two_factor: bool,
    pub password: ImportedPassword,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImportedPassword {
    /// Kuma's bcrypt hash, replaced with argon2 at the first sign-in.
    Kept,
    /// Inactive in Kuma or a legacy hash: an instance admin sets a new password.
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportInventoryPage {
    pub items: Vec<ImportInventoryItem>,
    /// `cursor` for the next page; absent on the last one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ImportInventoryItem {
    /// `monitor` (imported paused), `channel`, `custom_css`, `api_key`, `docker_host` and so on.
    pub kind: String,
    /// A name to recognise it by, never a secret.
    pub label: String,
    pub reason: String,
    pub source_table: String,
    pub source_id: i64,
    /// Source columns kept with it; their values are sealed.
    pub columns: Vec<String>,
}
