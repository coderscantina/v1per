//! Probes on the node tree (decision 0003, milestone 06). The built-in probe sits on the root;
//! remote probe agents are registered on any node and serve monitors of that node's subtree.
//! Instance-wide probes live on the root. Registering and revoking needs `probes.manage`,
//! reading `node.read`.
//!
//! Changes are recorded on the node's `probes:<id>` topic as `probe.created`, `probe.revoked`,
//! `probe.online` and `probe.offline`, each with the probe's ID. Health figures change with every
//! agent contact and record no event; read them again while a screen shows them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{NodeId, ProbeId};
use crate::operations::nodes::NodeArgs;

crate::domain! {
    pub trait ProbesHandlers;
    commands {
        /// Registers a remote probe on `node` and discloses its token once. Needs
        /// `probes.manage`. Run the agent with `v1per probe --server <url> --token <secret>`.
        "probes.create" => probes_create (Person, secret): ProbesCreateArgs => ProbeCreated;
        /// Revokes a probe's token at once: the agent's next request is refused and the probe
        /// leaves every monitor it served. Needs `probes.manage` on the probe's node. The
        /// built-in probe cannot be revoked (`invalid_value` on `probe`). Revoking a revoked
        /// probe answers it unchanged.
        "probes.revoke" => probes_revoke (Person): ProbeArgs => ProbeResult;
    }
    queries {
        /// The probes registered on one node with their health, oldest first, revoked ones
        /// included. Needs `node.read`. On the `probes:<id>` topic.
        "probes.list" => probes_list (Person, key): NodeArgs => ProbeList;
        /// The probes a monitor on `node` may use: those registered on the node and its
        /// ancestors, not revoked, nearest node first, with their node where the caller reads
        /// it. Needs `node.read`. On the node's
        /// `probes:<id>` topic, which does not record changes on ancestors.
        "probes.available" => probes_available (Person, key): NodeArgs => ProbeChoiceList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbesCreateArgs {
    pub node: NodeId,
    /// 1 to 100 characters after trimming.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeArgs {
    pub probe: ProbeId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProbeStatus {
    /// Registered, but the agent has not connected yet.
    Waiting,
    Online,
    /// No contact for 90 seconds. Its results no longer count in any quorum.
    Offline,
    Revoked,
}

/// A probe with its health as its agent last reported it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Probe {
    pub id: ProbeId,
    pub node: NodeId,
    pub name: String,
    /// The server's own probe, on the root.
    pub builtin: bool,
    pub status: ProbeStatus,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub revoked_at: Option<i64>,
    /// Last contact from the agent; absent before the first and for the built-in probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<i64>,
    /// The agent's v1per version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    /// Server clock minus agent clock at the last contact, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub clock_skew_ms: Option<i64>,
    /// The skew is above 5 seconds: heartbeat times from this probe are off by about as much.
    pub clock_skewed: bool,
    /// Checks started since the agent started, and those that started more than one interval
    /// late.
    pub checks_started: u64,
    pub checks_late: u64,
    /// Results the agent holds because it could not deliver them yet.
    pub buffered_results: u64,
    /// Results the agent dropped because its buffer was full, since its buffer was created.
    pub dropped_results: u64,
    /// Monitors assigned to it.
    pub monitors: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeResult {
    pub probe: Probe,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeCreated {
    pub probe: Probe,
    /// The agent's token. `null` in a replayed answer.
    pub secret: Option<String>,
    /// True when this answer is a replay and the token was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeList {
    pub probes: Vec<Probe>,
}

/// A probe a monitor may use.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeChoice {
    pub id: ProbeId,
    pub name: String,
    pub builtin: bool,
    pub status: ProbeStatus,
    /// Registered on the node itself rather than on an ancestor.
    pub own: bool,
    /// The node it is registered on; absent when the caller cannot read that node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub node: Option<NodeId>,
    /// Last contact from the agent; absent before the first and for the built-in probe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub last_seen_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeChoiceList {
    pub probes: Vec<ProbeChoice>,
}
