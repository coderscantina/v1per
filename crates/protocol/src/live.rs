//! Frames of `GET /api/subscribe?topic=<topic>[@<seq>]&...`, each sent as the `data` of a
//! server-sent event (architecture patterns, Topics, cursors and subscriptions).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{
    ChannelId, ComponentId, DeliveryId, IncidentId, MaintenanceId, MonitorId, PageId, ProbeId,
};

/// A frame of `/api/subscribe`, which may carry several topics: the frame of one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TopicFrame {
    pub topic: String,
    #[serde(flatten)]
    pub frame: LiveFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveFrame {
    /// The stream is open; `head` is the topic's current position.
    Subscribed { head: String, cursor: String },
    /// Events after the last cursor, in sequence order; `cursor` is the position after the last.
    Events {
        cursor: String,
        events: Vec<LiveEvent>,
    },
    /// Subscribe again from `cursor`; nothing after it was delivered. The topic ends.
    Resync {
        cursor: String,
        reason: ResyncReason,
    },
    /// The cursor is not on this topic's history: read a fresh snapshot. The topic ends.
    Expired { head: String },
}

/// One state event of a topic. Read the snapshot again to see what changed; the ID fields say
/// which resource it was about, so a screen showing another can skip the read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct LiveEvent {
    pub seq: i64,
    /// What happened, as `<resource>.<change>`.
    pub kind: String,
    /// The monitor of a `monitor.*` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<MonitorId>,
    /// The probe of a `probe.*` or `monitor.probe_changed` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
    /// The channel of a `channel.*` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<ChannelId>,
    /// The delivery of a `channel.delivery` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivery: Option<DeliveryId>,
    /// The incident of an `incident.*` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub incident: Option<IncidentId>,
    /// The maintenance of a `maintenance.*` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub maintenance: Option<MaintenanceId>,
    /// The component of a `component.*` event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub component: Option<ComponentId>,
    /// The page of a `page.*` event on a `pages:<id>` topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub page: Option<PageId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResyncReason {
    /// The session or the access to the topic is gone, or could not be checked just now.
    Unavailable,
}
