//! The node tree (decision 0001). Every operation resolves the caller's scope on a node first:
//! a node the caller cannot read answers `not_found`, whether or not it exists, and a readable
//! node without the needed capability answers `forbidden`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::{Capability, Role};
use crate::envelope::Empty;
use crate::ids::NodeId;

crate::domain! {
    pub trait NodesHandlers;
    commands {
        /// Creates a node as the last child of `parent`. Needs `node.write` on the parent. A
        /// node sits at most 8 levels below the root (`too_many` on `parent`).
        "nodes.create" => nodes_create (Person, key): NodesCreateArgs => NodeResult;
        /// Renames a node. Needs `node.write`.
        "nodes.rename" => nodes_rename (Person, key): NodesRenameArgs => NodeResult;
        /// Moves a node to `position` among its siblings, counted from 0; past the end means
        /// last. Needs `node.write` on the parent.
        "nodes.reorder" => nodes_reorder (Person, key): NodesReorderArgs => NodeResult;
        /// Moves a node with its subtree to the end of another parent. Needs `node.move` on the
        /// node and `node.write` on the current and the new parent. The root cannot move, a node
        /// cannot move below itself (`invalid_value` on `parent`) and the subtree must stay
        /// within 8 levels (`too_many` on `parent`). Who sees the subtree changes with it.
        "nodes.move" => nodes_move (Person): NodesMoveArgs => NodeResult;
        /// Deletes a node. Needs `node.delete` and reauthentication. A node with descendants
        /// needs `descendants` set to their number, else `node_not_empty` with that number in
        /// `current`; then the whole subtree with its grants, invitations and API keys goes.
        /// The root cannot be deleted.
        "nodes.delete" => nodes_delete (Person, reauth): NodesDeleteArgs => NodeDeleted;
        /// Requires 2FA of everyone holding a grant on the node or below it, or lifts that
        /// requirement. Needs `members.manage` and reauthentication. Sessions without a second
        /// factor become enrollment-only at their next request.
        "nodes.requireTwoFactor" => nodes_require_two_factor (Person, reauth): NodesRequireTwoFactorArgs => NodeResult;
    }
    queries {
        /// The tree as the caller sees it, depth first with siblings in order: every node the
        /// caller can read, and the ancestors above them as placeholders without a name. On the
        /// caller's `user:<id>` topic, which records every change to what they see.
        "nodes.tree" => nodes_tree (Person, key): Empty => NodeTree;
        /// One node with its path. Needs `node.read`. On the `node:<id>` topic.
        "nodes.get" => nodes_get (Person, key): NodeArgs => NodeResult;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodeArgs {
    pub node: NodeId,
}

/// A node as the caller sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Node {
    pub id: NodeId,
    /// Absent for the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent: Option<NodeId>,
    pub name: String,
    /// Levels below the root; the root is 0.
    pub depth: u32,
    /// Place among its siblings, from 0.
    pub position: u32,
    /// Ancestors from the root down to the parent.
    pub path: Vec<PathEntry>,
    /// The caller's effective role: the highest grant on the node or an ancestor.
    pub role: Role,
    /// What the caller may do here. An API key holds fewer than its owner's role.
    pub capabilities: Vec<Capability>,
    /// This node requires 2FA.
    pub two_factor_required: bool,
    /// An ancestor requires 2FA, which applies here too.
    pub two_factor_inherited: bool,
    pub children: u32,
    pub created_at: i64,
}

/// One ancestor in a node's path. Ancestors above the caller's highest grant carry no name.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PathEntry {
    pub id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodeResult {
    pub node: Node,
}

/// One entry of `nodes.tree`. A placeholder (an ancestor the caller cannot read) has neither
/// `name` nor `role`, and no capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct TreeNode {
    pub id: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parent: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    pub depth: u32,
    pub position: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub role: Option<Role>,
    pub capabilities: Vec<Capability>,
    pub two_factor_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodeTree {
    pub nodes: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesCreateArgs {
    pub parent: NodeId,
    /// 1 to 100 characters after trimming.
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesRenameArgs {
    pub node: NodeId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesReorderArgs {
    pub node: NodeId,
    pub position: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesMoveArgs {
    pub node: NodeId,
    /// The new parent.
    pub parent: NodeId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesDeleteArgs {
    pub node: NodeId,
    /// The number of descendant nodes the caller confirms deleting with the node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub descendants: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodeDeleted {
    pub node: NodeId,
    /// Descendant nodes deleted with it.
    pub descendants: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NodesRequireTwoFactorArgs {
    pub node: NodeId,
    pub required: bool,
}
