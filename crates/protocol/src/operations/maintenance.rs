//! Planned maintenance (decision 0004, monitoring rules 0008 item 9, milestone 08). A
//! maintenance belongs to a node and covers monitors on that node and below. While one of its
//! windows is active the covered monitors, and the members of covered groups, skip their
//! checks, record status `maintenance` every interval and send no alerts.
//!
//! Schedules follow Uptime Kuma 2.5.0's six strategies, with Kuma's field names. Dates and
//! times are wall clock in the maintenance's `timezone`, or the instance timezone when it is
//! absent: `YYYY-MM-DDTHH:MM` for dates, `HH:MM` for times. Reading needs `node.read`, changes
//! `maintenance.manage`.
//!
//! Events go to the `maintenance:<id>` topic of the maintenance's node and every ancestor:
//! `maintenance.created`, `maintenance.updated`, `maintenance.deleted`, and
//! `maintenance.status_changed` when a window starts or ends, each with its ID.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{MaintenanceId, MonitorId, NodeId};

crate::domain! {
    pub trait MaintenanceHandlers;
    commands {
        /// Plans an active maintenance on `node`. `monitors` must be on the node or below
        /// (`invalid_value` on `monitors`). Needs `maintenance.manage`.
        "maintenance.create" => maintenance_create (Person, key): MaintenanceCreateArgs => MaintenanceResult;
        /// Replaces a maintenance's title, schedule and monitors. Needs `maintenance.manage`.
        "maintenance.update" => maintenance_update (Person, key): MaintenanceUpdateArgs => MaintenanceResult;
        /// Stops a maintenance: no window applies until it resumes. Ends a `manual` one.
        "maintenance.pause" => maintenance_pause (Person, key): MaintenanceArgs => MaintenanceResult;
        /// Lets a paused maintenance's windows apply again. Starts a `manual` one.
        "maintenance.resume" => maintenance_resume (Person, key): MaintenanceArgs => MaintenanceResult;
        /// Deletes a maintenance. Needs `maintenance.manage`.
        "maintenance.delete" => maintenance_delete (Person, key): MaintenanceArgs => MaintenanceDeleted;
    }
    queries {
        /// The maintenances of a node, or with `subtree` of the node and every node below it,
        /// by title, each with its status and next three windows. Needs `node.read`. On the
        /// node's `maintenance:<id>` topic.
        "maintenance.list" => maintenance_list (Person, key): MaintenanceListArgs => MaintenanceList;
        /// One maintenance with its next ten windows. Needs `node.read`.
        "maintenance.get" => maintenance_get (Person, key): MaintenanceArgs => MaintenanceDetail;
        /// The next ten windows of a schedule that is not saved yet, for the editor. Invalid
        /// fields answer `invalid_arguments`. On the `instance` topic.
        "maintenance.preview" => maintenance_preview (Person, key): MaintenanceSchedule => MaintenancePreview;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MaintenanceStrategy {
    /// Active until paused.
    Manual,
    /// From `start_date` to `end_date`.
    Single,
    /// From `start_time` to `end_time` every `interval_day` days, counted from `start_date`.
    RecurringInterval,
    /// From `start_time` to `end_time` on `weekdays`.
    RecurringWeekday,
    /// From `start_time` to `end_time` on `days_of_month`.
    RecurringDayOfMonth,
    /// For `duration` seconds from each `cron` firing.
    Cron,
}

/// When a maintenance applies. Which fields count depends on `strategy`; the recurring
/// strategies and `cron` run only between `start_date` and `end_date` when those are set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceSchedule {
    pub strategy: MaintenanceStrategy,
    /// `YYYY-MM-DDTHH:MM` local time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_date: Option<String>,
    /// `YYYY-MM-DDTHH:MM` local time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub end_date: Option<String>,
    /// `HH:MM`. An end before the start runs past midnight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start_time: Option<String>,
    /// `HH:MM`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub end_time: Option<String>,
    /// 0 to 6, Sunday is 0.
    #[serde(default)]
    #[ts(as = "Option<Vec<u8>>", optional)]
    pub weekdays: Vec<u8>,
    /// 1 to 31.
    #[serde(default)]
    #[ts(as = "Option<Vec<u8>>", optional)]
    pub days_of_month: Vec<u8>,
    /// The last day of every month too.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub last_day_of_month: bool,
    /// 1 to 3,650.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub interval_day: Option<u32>,
    /// Five fields, or six with seconds first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cron: Option<String>,
    /// Seconds each `cron` window lasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub duration: Option<u32>,
    /// An IANA zone such as `Europe/Vienna`; absent means the instance timezone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceInput {
    /// 1 to 200 characters after trimming.
    pub title: String,
    /// Up to 10,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub description: String,
    pub schedule: MaintenanceSchedule,
    /// The monitors it covers; members of a covered group are covered too.
    pub monitors: Vec<MonitorId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub maintenance: MaintenanceInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceUpdateArgs {
    pub maintenance: MaintenanceId,
    /// The maintenance's `revision` the edit is based on; `stale_revision` with the current
    /// one otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub input: MaintenanceInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceArgs {
    pub maintenance: MaintenanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceListArgs {
    pub node: NodeId,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
}

/// Kuma's maintenance statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceStatus {
    /// Paused.
    Inactive,
    /// Waiting for its next window.
    Scheduled,
    UnderMaintenance,
    /// Past its end date.
    Ended,
    /// The schedule produces no windows.
    Unknown,
}

/// A window, `[start, end)` in Unix milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceWindow {
    /// Absent for a `single` maintenance without a start date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start: Option<i64>,
    /// Absent for a `single` maintenance without an end date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub end: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Maintenance {
    pub id: MaintenanceId,
    pub node: NodeId,
    pub title: String,
    pub description: String,
    pub schedule: MaintenanceSchedule,
    pub monitors: Vec<MonitorId>,
    /// False while paused.
    pub active: bool,
    pub status: MaintenanceStatus,
    /// The current window first while one runs. `manual` has none.
    pub windows: Vec<MaintenanceWindow>,
    /// Rises with every change. Pass it as `expected_revision` to `maintenance.update`.
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceResult {
    pub maintenance: Maintenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceDeleted {
    pub maintenance: MaintenanceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceList {
    pub maintenances: Vec<Maintenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenanceDetail {
    pub maintenance: Maintenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MaintenancePreview {
    pub windows: Vec<MaintenanceWindow>,
}
