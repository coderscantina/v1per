//! Roles and capabilities on the node tree (decision 0001, `docs/contracts/capability-matrix.md`).
//!
//! A grant gives one [`Role`] on a node and its whole subtree. The effective role on a node is
//! the highest grant on it or any ancestor; the instance admin acts as owner of the root. The
//! server derives the [`Capability`] set from the effective role; the browser only displays it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Owner,
    Admin,
    Member,
    Viewer,
}

/// What a node scope allows. A closed set: every node-scoped operation names one.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
pub enum Capability {
    /// See the node, its subtree, members, monitors, status pages and incidents.
    #[serde(rename = "node.read")]
    NodeRead,
    #[serde(rename = "monitors.manage")]
    MonitorsManage,
    #[serde(rename = "incidents.manage")]
    IncidentsManage,
    #[serde(rename = "maintenance.manage")]
    MaintenanceManage,
    #[serde(rename = "pages.manage")]
    PagesManage,
    #[serde(rename = "channels.manage")]
    ChannelsManage,
    #[serde(rename = "probes.manage")]
    ProbesManage,
    /// Rename the node, create and order its children.
    #[serde(rename = "node.write")]
    NodeWrite,
    /// Invite, change and remove grants under the rank rule, and require 2FA.
    #[serde(rename = "members.manage")]
    MembersManage,
    /// Move the node to another parent.
    #[serde(rename = "node.move")]
    NodeMove,
    /// Delete the node and, with confirmation, its subtree.
    #[serde(rename = "node.delete")]
    NodeDelete,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Owner, Role::Admin, Role::Member, Role::Viewer];

    /// Owner 3, admin 2, member 1, viewer 0. The effective role is the highest rank on the path.
    pub const fn rank(self) -> u8 {
        match self {
            Role::Owner => 3,
            Role::Admin => 2,
            Role::Member => 1,
            Role::Viewer => 0,
        }
    }

    pub fn from_rank(rank: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|role| i64::from(role.rank()) == rank)
    }

    /// The capabilities this role grants, in declaration order.
    pub fn capabilities(self) -> &'static [Capability] {
        use Capability::*;
        match self {
            Role::Viewer => &[NodeRead],
            Role::Member => &[NodeRead, MonitorsManage, IncidentsManage, MaintenanceManage],
            Role::Admin => &[
                NodeRead,
                MonitorsManage,
                IncidentsManage,
                MaintenanceManage,
                PagesManage,
                ChannelsManage,
                ProbesManage,
                NodeWrite,
                MembersManage,
            ],
            Role::Owner => &[
                NodeRead,
                MonitorsManage,
                IncidentsManage,
                MaintenanceManage,
                PagesManage,
                ChannelsManage,
                ProbesManage,
                NodeWrite,
                MembersManage,
                NodeMove,
                NodeDelete,
            ],
        }
    }

    /// R-RANK, checked for both the role someone holds and the role they get: an actor
    /// changes, removes and grants only roles below its own, except that owners may make and
    /// change owners.
    pub fn may_assign(self, role: Role) -> bool {
        role.rank() < self.rank() || (self == Role::Owner && role == Role::Owner)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Member => "member",
            Role::Viewer => "viewer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.as_str() == value)
    }
}

impl Capability {
    pub const ALL: [Capability; 11] = [
        Capability::NodeRead,
        Capability::MonitorsManage,
        Capability::IncidentsManage,
        Capability::MaintenanceManage,
        Capability::PagesManage,
        Capability::ChannelsManage,
        Capability::ProbesManage,
        Capability::NodeWrite,
        Capability::MembersManage,
        Capability::NodeMove,
        Capability::NodeDelete,
    ];

    /// Whether an API key may hold it. Who may access a node and whether the node exists stay
    /// with people.
    pub const fn key_grantable(self) -> bool {
        !matches!(
            self,
            Capability::MembersManage | Capability::NodeMove | Capability::NodeDelete
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::NodeRead => "node.read",
            Capability::MonitorsManage => "monitors.manage",
            Capability::IncidentsManage => "incidents.manage",
            Capability::MaintenanceManage => "maintenance.manage",
            Capability::PagesManage => "pages.manage",
            Capability::ChannelsManage => "channels.manage",
            Capability::ProbesManage => "probes.manage",
            Capability::NodeWrite => "node.write",
            Capability::MembersManage => "members.manage",
            Capability::NodeMove => "node.move",
            Capability::NodeDelete => "node.delete",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_their_wire_form() {
        for capability in Capability::ALL {
            let wire = serde_json::to_value(capability).unwrap();
            assert_eq!(wire, capability.as_str());
        }
        for role in Role::ALL {
            assert_eq!(serde_json::to_value(role).unwrap(), role.as_str());
            assert_eq!(Role::from_rank(role.rank().into()), Some(role));
        }
    }

    /// Each role holds everything the role below it holds.
    #[test]
    fn higher_roles_hold_every_lower_capability() {
        for pair in Role::ALL.windows(2) {
            let (higher, lower) = (pair[0], pair[1]);
            for capability in lower.capabilities() {
                assert!(
                    higher.capabilities().contains(capability),
                    "{higher:?} {capability:?}"
                );
            }
        }
    }
}
