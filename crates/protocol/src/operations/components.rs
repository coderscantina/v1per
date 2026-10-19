//! Static components: parts of a service without a monitor, such as "Email delivery", that
//! incidents name and status pages show (decision 0004). A component belongs to a node and is
//! usable in its subtree. Reading needs `node.read`, changes `pages.manage`.
//!
//! Events go to the `components:<id>` topic of the component's node and every ancestor:
//! `component.created`, `component.updated` and `component.deleted` with its ID.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{ComponentId, NodeId};

crate::domain! {
    pub trait ComponentsHandlers;
    commands {
        /// Creates a static component on `node`. Needs `pages.manage`.
        "components.create" => components_create (Person): ComponentsCreateArgs => ComponentResult;
        /// Renames or describes a component. Needs `pages.manage` on its node.
        "components.update" => components_update (Person): ComponentsUpdateArgs => ComponentResult;
        /// Deletes a component; incidents stop naming it. Needs `pages.manage`.
        "components.delete" => components_delete (Person): ComponentArgs => ComponentDeleted;
    }
    queries {
        /// The components of a node, or with `subtree` of the node and every node below it, by
        /// name. Needs `node.read`. On the node's `components:<id>` topic.
        "components.list" => components_list (Person, key): ComponentsListArgs => ComponentList;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentInput {
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// Up to 1,000 characters.
    #[serde(default)]
    #[ts(as = "Option<String>", optional)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentsCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub component: ComponentInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentsUpdateArgs {
    pub component: ComponentId,
    #[serde(flatten)]
    pub input: ComponentInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentArgs {
    pub component: ComponentId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentsListArgs {
    pub node: NodeId,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Component {
    pub id: ComponentId,
    pub node: NodeId,
    pub name: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentResult {
    pub component: Component,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentDeleted {
    pub component: ComponentId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentList {
    pub components: Vec<Component>,
}
