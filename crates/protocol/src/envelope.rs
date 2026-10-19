//! The fields every command and query answer shares with its operation-specific result.
//!
//! A command body is `CommandKey & Args`; its answer is `CommandMeta & Result`. A query answer is
//! `SnapshotMeta & Result`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CommandKey {
    /// Repeating a key repeats the same intent: the stored answer comes back without running
    /// the command again. 1 to 128 characters, kept for 24 hours.
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct CommandMeta {
    /// Sequence of the last topic event the command recorded, or the `instance` head when it
    /// recorded none.
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SnapshotMeta {
    /// Head of the snapshot's topic, read in the same transaction as the data. Pass it as
    /// `expected_revision` to a command that changes the same resource.
    pub revision: i64,
    /// `<topic>@<revision>`. Opaque to the browser; subscribe from it to follow the snapshot.
    pub cursor: String,
}

/// Arguments of operations that take none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Empty {}

/// A named, ordered log of state events: `instance`, `user:<id>`, and per node one topic for
/// each aggregate, so an edit conflicts only with edits of the same thing: `node:<id>` (the
/// node itself and its children), `members:<id>` (grants and invitations), `monitors:<id>`
/// (its monitors and their status), `monitor-tree:<id>` (the monitors of the node and every
/// node below it), `probes:<id>` (the probes registered on it), `channels:<id>` (its
/// notification channels and their deliveries), and `incidents:<id>`, `maintenance:<id>` and
/// `components:<id>` (those of the node and every node below it), `pages:<id>` (the status
/// pages of the node and every node below it) and, per status page, `page:<id>`: everything
/// its snapshot shows, which the public page stream follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic(String);

impl Topic {
    pub fn instance() -> Self {
        Self("instance".into())
    }

    pub fn user(user: &crate::ids::UserId) -> Self {
        Self(format!("user:{user}"))
    }

    pub fn node(node: crate::ids::NodeId) -> Self {
        Self(format!("node:{node}"))
    }

    pub fn members(node: crate::ids::NodeId) -> Self {
        Self(format!("members:{node}"))
    }

    pub fn monitors(node: crate::ids::NodeId) -> Self {
        Self(format!("monitors:{node}"))
    }

    pub fn monitor_tree(node: crate::ids::NodeId) -> Self {
        Self(format!("monitor-tree:{node}"))
    }

    pub fn probes(node: crate::ids::NodeId) -> Self {
        Self(format!("probes:{node}"))
    }

    pub fn channels(node: crate::ids::NodeId) -> Self {
        Self(format!("channels:{node}"))
    }

    pub fn incidents(node: crate::ids::NodeId) -> Self {
        Self(format!("incidents:{node}"))
    }

    pub fn maintenance(node: crate::ids::NodeId) -> Self {
        Self(format!("maintenance:{node}"))
    }

    pub fn components(node: crate::ids::NodeId) -> Self {
        Self(format!("components:{node}"))
    }

    pub fn pages(node: crate::ids::NodeId) -> Self {
        Self(format!("pages:{node}"))
    }

    pub fn page(page: crate::ids::PageId) -> Self {
        Self(format!("page:{page}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn cursor(&self, revision: i64) -> String {
        format!("{}@{revision}", self.0)
    }
}
