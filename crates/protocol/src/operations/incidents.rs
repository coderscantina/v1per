//! Incidents (decision 0004, monitoring contract "Incident impact", milestone 08). One entity
//! serves manual incidents and auto-incidents from failing monitors. An incident belongs to a
//! node and names affected monitors and static components from that node's subtree. Its
//! timeline holds updates with a state, a message and an impact per named component; the
//! latest update that names a component sets its impact, and a resolved incident has every
//! component operational.
//!
//! A monitor's down transition opens an auto-incident on the monitor's node unless one is still
//! open for it. Recovery resolves it unless a person has changed it (`edited`). Reading needs
//! `node.read`, everything else `incidents.manage`.
//!
//! Publication is per status page (milestone 09). A manual incident is published at creation
//! to the pages showing its targets on which its author manages incidents. An auto-incident
//! follows each page's mode: `draft` waits for a person, `auto_publish` publishes it once it
//! stayed open for the page's grace. Pages never show unpublished incidents.
//!
//! Events go to the `incidents:<id>` topic of the incident's node and every ancestor:
//! `incident.created`, `incident.updated` and `incident.deleted`, each with the incident's ID.
//! Publishing records `incident.updated`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{ComponentId, IncidentId, IncidentUpdateId, MonitorId, NodeId, PageId};

crate::domain! {
    pub trait IncidentsHandlers;
    commands {
        /// Opens an incident on `node` with its first update. Every target in `impacts` must
        /// be a monitor or static component on the node or below (`invalid_value` on
        /// `impacts`). Needs `incidents.manage`.
        "incidents.create" => incidents_create (Person, key): IncidentsCreateArgs => IncidentResult;
        /// Adds an update to the timeline. Targets it names that the incident did not affect
        /// yet are added. `resolved` ends the incident; another state reopens it, unless it is
        /// an auto-incident whose monitor has a newer open one (`invalid_value` on `state`).
        /// Marks an auto-incident as edited. Needs `incidents.manage`.
        "incidents.addUpdate" => incidents_add_update (Person, key): IncidentsAddUpdateArgs => IncidentResult;
        /// Changes the title and the affected targets. A target taken out loses its impact; one
        /// added has none until an update names it. Marks an auto-incident as edited. Needs
        /// `incidents.manage`.
        "incidents.update" => incidents_update (Person, key): IncidentsUpdateArgs => IncidentResult;
        /// Deletes an incident with its timeline, such as an auto-incident dismissed as a false
        /// alarm. Needs `incidents.manage`.
        "incidents.delete" => incidents_delete (Person, key): IncidentArgs => IncidentDeleted;
        /// Publishes the incident to a page that shows at least one of its targets
        /// (`invalid_value` on `page` otherwise). Needs `incidents.manage` on the incident's
        /// node and on the page's.
        "incidents.publish" => incidents_publish (Person, key): IncidentPublishArgs => IncidentPublications;
        /// Takes the incident off a page, and stops a pending auto-publication there. Needs
        /// `incidents.manage` on the incident's node and on the page's.
        "incidents.unpublish" => incidents_unpublish (Person, key): IncidentPublishArgs => IncidentPublications;
    }
    queries {
        /// The incidents of a node, or with `subtree` of the node and every node below it,
        /// newest first. Needs `node.read`. On the node's `incidents:<id>` topic.
        "incidents.list" => incidents_list (Person, key): IncidentsListArgs => IncidentPage;
        /// One incident with its affected targets and its timeline, oldest update first. Needs
        /// `node.read`. On the incident node's `incidents:<id>` topic.
        "incidents.get" => incidents_get (Person, key): IncidentArgs => IncidentDetail;
        /// The pages the caller reads that show at least one of the incident's targets, and
        /// whether it is published there or due to be. Needs `node.read`. On the incident
        /// node's `incidents:<id>` topic.
        "incidents.publications" => incidents_publications (Person, key): IncidentArgs => IncidentPublications;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentState {
    Investigating,
    Identified,
    Monitoring,
    Resolved,
}

/// How badly a component is affected, from best to worst.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    Operational,
    Degraded,
    PartialOutage,
    MajorOutage,
}

/// A monitor or a static component an incident names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    Monitor { monitor: MonitorId },
    Component { component: ComponentId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TargetImpact {
    pub target: Target,
    pub impact: Impact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentsCreateArgs {
    pub node: NodeId,
    /// 1 to 200 characters after trimming.
    pub title: String,
    /// Default `investigating`.
    #[serde(default = "investigating")]
    #[ts(as = "Option<IncidentState>", optional)]
    pub state: IncidentState,
    /// Up to 10,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub message: String,
    /// The affected targets and their impact, each target once.
    pub impacts: Vec<TargetImpact>,
}

fn investigating() -> IncidentState {
    IncidentState::Investigating
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentsAddUpdateArgs {
    pub incident: IncidentId,
    pub state: IncidentState,
    /// Up to 10,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub message: String,
    /// Targets whose impact changes, each once.
    #[serde(default)]
    #[ts(as = "Option<Vec<TargetImpact>>", optional)]
    pub impacts: Vec<TargetImpact>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentsUpdateArgs {
    pub incident: IncidentId,
    /// The incident's `revision` the edit is based on; `stale_revision` with the current one
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    /// 1 to 200 characters after trimming.
    pub title: String,
    pub affected: Vec<Target>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentArgs {
    pub incident: IncidentId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentStatusFilter {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncidentOrigin {
    /// Opened by a monitor's down transition.
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentsListArgs {
    pub node: NodeId,
    /// The incidents of every node below too.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
    /// Open or resolved only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<IncidentStatusFilter>,
    /// Auto-incidents or manual ones only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub origin: Option<IncidentOrigin>,
    /// Only incidents that affect this monitor.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<MonitorId>,
    /// `next_cursor` of the page before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cursor: Option<String>,
    /// 1 to 100, default 25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Incident {
    pub id: IncidentId,
    pub node: NodeId,
    pub title: String,
    /// The latest update's state.
    pub state: IncidentState,
    /// The worst impact among the affected targets; absent while none has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub impact: Option<Impact>,
    pub origin: IncidentOrigin,
    /// The monitor whose down transition opened it, while that monitor exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auto_monitor: Option<MonitorId>,
    /// A person changed this auto-incident, so recovery no longer resolves it.
    pub edited: bool,
    /// Affected monitors and components.
    pub affected: u32,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_at: Option<i64>,
    pub updated_at: i64,
    /// Rises with every change. Pass it as `expected_revision` to `incidents.update`.
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentResult {
    pub incident: Incident,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentDeleted {
    pub incident: IncidentId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentPage {
    pub incidents: Vec<Incident>,
    /// `cursor` for the next page; absent on the last one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_cursor: Option<String>,
}

/// An affected target with its current impact.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AffectedTarget {
    pub target: Target,
    pub name: String,
    /// Absent until an update names the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub impact: Option<Impact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentUpdate {
    pub id: IncidentUpdateId,
    pub state: IncidentState,
    pub message: String,
    pub impacts: Vec<TargetImpact>,
    /// The person who wrote it; absent for updates the server wrote for an auto-incident.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub author: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentDetail {
    pub incident: Incident,
    pub affected: Vec<AffectedTarget>,
    /// Oldest first.
    pub updates: Vec<IncidentUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentPublishArgs {
    pub incident: IncidentId,
    pub page: PageId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentPublication {
    pub page: PageId,
    pub slug: String,
    pub title: String,
    /// When it was published there; absent while it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub published_at: Option<i64>,
    /// When the page's `auto_publish` grace publishes it, while pending.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scheduled_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct IncidentPublications {
    pub pages: Vec<IncidentPublication>,
}
