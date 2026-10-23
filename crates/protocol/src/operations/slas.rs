//! SLAs (decision 0012, milestone 14, monitoring contract "SLAs"). An SLA belongs to a node and
//! covers monitors in that node's subtree. It states up to four commitments, each optional:
//! availability, response and resolution time per impact, and average latency. Periods are
//! calendar months, quarters or years in the SLA's timezone, from its start date on.
//!
//! Reading needs `node.read` on the SLA's node. Creating, changing, deleting and recomputing
//! need Admin or Owner through a grant on a strict ancestor of the node, or the instance
//! admin: a client Admin on the SLA's own node reads only (`forbidden` on writes).
//!
//! Events go to the `slas:<id>` topic of the SLA's node and every ancestor: `sla.created`,
//! `sla.updated`, `sla.deleted` and `sla.period_frozen`, each with the SLA's ID.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{ChannelId, IncidentId, MonitorId, NodeId, SlaId};
use crate::operations::incidents::Impact;

crate::domain! {
    pub trait SlasHandlers;
    commands {
        /// Creates an SLA on `node`. `monitors` must be on the node or below and `channels` on
        /// the node or an ancestor (`invalid_value` on the field). Needs Admin through a grant
        /// on an ancestor of `node`, or the instance admin.
        "slas.create" => slas_create (Person): SlasCreateArgs => SlaResult;
        /// Replaces an SLA's terms. Frozen periods keep the terms they were computed with.
        /// Needs Admin through a grant on an ancestor of the SLA's node.
        "slas.update" => slas_update (Person): SlasUpdateArgs => SlaResult;
        /// Deletes an SLA with its frozen periods and tallies, and takes it off status pages.
        /// Needs Admin through a grant on an ancestor of the SLA's node.
        "slas.delete" => slas_delete (Person): SlaArgs => SlaDeleted;
        /// Computes a closed period again from the current terms and data and freezes the
        /// result in place of the old one. Needs Admin through a grant on an ancestor of the
        /// SLA's node.
        "slas.recompute" => slas_recompute (Person): SlaPeriodArgs => SlaPeriodResult;
    }
    queries {
        /// The SLAs of a node, or with `subtree` of the node and every node below it, by name.
        /// Needs `node.read`. On the node's `slas:<id>` topic.
        "slas.list" => slas_list (Person): SlasListArgs => SlaList;
        /// One SLA. Needs `node.read` on its node. On the node's `slas:<id>` topic.
        "slas.get" => slas_get (Person): SlaArgs => SlaDetail;
        /// One period of an SLA: frozen when it closed more than an hour ago, computed now
        /// otherwise. Needs `node.read` on the SLA's node. On the node's `slas:<id>` topic.
        "slas.period" => slas_period (Person): SlaPeriodArgs => SlaPeriodResult;
    }
}

/// How long one period lasts, in the SLA's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlaPeriodKind {
    Month,
    Quarter,
    Year,
}

/// How the availability target applies to several monitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlaCombine {
    /// To every covered monitor on its own; the SLA misses when any monitor misses.
    Each,
    /// To one service timeline: a minute is down when any covered monitor is down in it.
    AnyDown,
}

/// Response and resolution targets for incidents of one impact, in business minutes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaTimeTarget {
    /// 1 to 525,600 minutes from start to the first update a person writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub response_minutes: Option<u32>,
    /// 1 to 525,600 minutes from start to resolution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolution_minutes: Option<u32>,
}

/// Time targets by the worst impact an incident reached on a covered monitor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaTimeTargets {
    #[serde(default)]
    #[ts(as = "Option<SlaTimeTarget>", optional)]
    pub major_outage: SlaTimeTarget,
    #[serde(default)]
    #[ts(as = "Option<SlaTimeTarget>", optional)]
    pub partial_outage: SlaTimeTarget,
    #[serde(default)]
    #[ts(as = "Option<SlaTimeTarget>", optional)]
    pub degraded: SlaTimeTarget,
}

/// Business hours on one weekday. Ranges of one weekday must not overlap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaHours {
    /// 0 to 6, Sunday is 0.
    pub weekday: u8,
    /// `HH:MM`.
    pub start: String,
    /// `HH:MM` after `start`, or `24:00` for the end of the day.
    pub end: String,
}

/// What `slas.create` and `slas.update` set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaInput {
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// An IANA zone such as `Europe/Vienna`. Periods, days and business hours are counted in it.
    pub timezone: String,
    pub period: SlaPeriodKind,
    /// `YYYY-MM-DD`, the contract start. Periods before the one holding it are not computed.
    pub start_date: String,
    pub combine: SlaCombine,
    /// Availability target in percent, above 0 and at most 100, up to three decimals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability: Option<f64>,
    /// Average latency target in milliseconds, 1 to 600,000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    #[serde(default)]
    #[ts(as = "Option<SlaTimeTargets>", optional)]
    pub times: SlaTimeTargets,
    /// Weekly business hours the time targets count. Empty: around the clock.
    #[serde(default)]
    #[ts(as = "Option<Vec<SlaHours>>", optional)]
    pub hours: Vec<SlaHours>,
    /// `YYYY-MM-DD` days without business hours, up to 1,000.
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub holidays: Vec<String>,
    /// 1 to 500 monitors on the SLA's node or below.
    pub monitors: Vec<MonitorId>,
    /// Channels on the SLA's node or an ancestor that get its warnings and breaches.
    #[serde(default)]
    #[ts(as = "Option<Vec<ChannelId>>", optional)]
    pub channels: Vec<ChannelId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlasCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub sla: SlaInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlasUpdateArgs {
    pub sla: SlaId,
    /// The SLA's `revision` the edit is based on; `stale_revision` with the current one
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub input: SlaInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaArgs {
    pub sla: SlaId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlasListArgs {
    pub node: NodeId,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Sla {
    pub id: SlaId,
    pub node: NodeId,
    #[serde(flatten)]
    pub terms: SlaInput,
    /// Rises with every change. Pass it as `expected_revision` to `slas.update`.
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaResult {
    pub sla: Sla,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaDeleted {
    pub sla: SlaId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaList {
    pub slas: Vec<Sla>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaDetail {
    pub sla: Sla,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaPeriodArgs {
    pub sla: SlaId,
    /// `YYYY-MM-DD`, the first day of the period. Absent: the running period. Required for
    /// `slas.recompute`, which takes closed periods only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub start: Option<String>,
}

/// How a commitment stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlaOutcome {
    Met,
    Missed,
    /// The period is running and the commitment is not missed yet.
    OnTrack,
    /// The data cannot support it, such as `any_down` days without minute rollups.
    NoData,
}

/// One covered monitor over the period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaMonitorStanding {
    pub monitor: MonitorId,
    /// The monitor's name when the period was computed.
    pub name: String,
    /// Up count over up and down count in percent, pending counted as down. Absent: no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability: Option<f64>,
    /// In `each` mode against the availability target; absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability_outcome: Option<SlaOutcome>,
    /// Share of this monitor's downtime budget used, 1 being all of it. `each` mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub budget_used: Option<f64>,
    /// Average latency of up beats in milliseconds. Absent for monitor types without latency
    /// and without data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<f64>,
    /// Against the latency target; absent without one or for types without latency.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_outcome: Option<SlaOutcome>,
}

/// How one incident clock stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlaClockOutcome {
    Met,
    Missed,
    /// Open and before its deadline.
    Running,
    /// No target for the incident's impact, or a response the resolution made unnecessary.
    NotCounted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaClock {
    pub outcome: SlaClockOutcome,
    /// Business milliseconds from start to the stop, or to now while running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub elapsed_ms: Option<i64>,
    /// When the target runs out, as an instant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub deadline: Option<i64>,
}

/// An incident that names a covered monitor and started in the period, drafts included.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaIncidentStanding {
    pub incident: IncidentId,
    pub title: String,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_at: Option<i64>,
    /// The worst impact any update set on a covered monitor.
    pub impact: Impact,
    pub response: SlaClock,
    pub resolution: SlaClock,
}

/// Counted incidents of one clock: those that met or missed it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaClockSummary {
    pub outcome: SlaOutcome,
    pub counted: u32,
    pub met: u32,
    /// Average business milliseconds of the counted incidents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub average_ms: Option<i64>,
}

/// The availability commitment over the period.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaAvailabilityStanding {
    pub outcome: SlaOutcome,
    /// In percent: the service timeline in `any_down` mode, the lowest monitor in `each`
    /// mode. Absent: no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub actual: Option<f64>,
    /// Share of the downtime budget used, 1 being all of it: the service's in `any_down`
    /// mode, the highest monitor's in `each` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub budget_used: Option<f64>,
}

/// One period of an SLA against the terms it was computed with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaPeriod {
    pub sla: SlaId,
    /// `YYYY-MM-DD`, the first day in the SLA's timezone.
    pub start: String,
    /// First instant of the period.
    pub starts_at: i64,
    /// First instant after the period.
    pub ends_at: i64,
    /// When the stored result was computed; absent while the period is computed live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub frozen_at: Option<i64>,
    /// The terms the result was computed with.
    pub terms: SlaInput,
    /// Absent without an availability target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability: Option<SlaAvailabilityStanding>,
    /// Absent without a latency target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency: Option<SlaOutcome>,
    /// Absent without any response target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub response: Option<SlaClockSummary>,
    /// Absent without any resolution target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolution: Option<SlaClockSummary>,
    pub monitors: Vec<SlaMonitorStanding>,
    /// By start time.
    pub incidents: Vec<SlaIncidentStanding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlaPeriodResult {
    pub period: SlaPeriod,
}
