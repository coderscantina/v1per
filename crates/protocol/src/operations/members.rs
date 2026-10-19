//! Who has access to a node: grants and invitations.
//!
//! Grants change under the rank rule (R-RANK): an actor changes, removes and grants only roles
//! below its own effective role on the node, except that owners may make and change owners.
//! Nobody changes their own grant; anyone may remove it. Invitations work without email until
//! SMTP arrives: the inviter receives a single-use link once and passes it on.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Role;
use crate::envelope::Empty;
use crate::ids::{InvitationId, NodeId, UserId};
use crate::operations::nodes::NodeArgs;

crate::domain! {
    pub trait MembersHandlers;
    commands {
        /// Creates a single-use invitation for one role on the node and discloses its link
        /// once. Needs `members.manage`, and the role must pass the rank rule. Lifetime 1 to 30
        /// days, 7 by default.
        "members.invite" => members_invite (Person, secret): MembersInviteArgs => InvitationCreated;
        /// Revokes a pending invitation. Needs `members.manage` on its node.
        "members.revokeInvitation" => members_revoke_invitation (Person): InvitationArgs => InvitationResult;
        /// Changes someone's grant on the node. Needs `members.manage`; the caller must outrank
        /// the person's effective role and the new role (R-RANK). Raising to admin or owner
        /// needs reauthentication.
        "members.updateRole" => members_update_role (Person, reauth_by_handler): MembersUpdateRoleArgs => MemberResult;
        /// Removes someone's grant on the node. Anyone may remove their own; removing another
        /// person's needs `members.manage` and the rank rule. Grants on ancestors stay.
        "members.remove" => members_remove (Person): MemberArgs => Empty;
    }
    queries {
        /// Everyone who can access the node: grants on it and its ancestors, and instance
        /// admins. Needs `node.read`. On the `members:<id>` topic, which also hears of grant
        /// changes on ancestors and of moves that change the ancestors.
        "members.list" => members_list (Person): NodeArgs => MemberList;
        /// Invitations to the node, newest first. Needs `members.manage`. On the `members:<id>`
        /// topic.
        "members.invitations" => members_invitations (Person): NodeArgs => InvitationList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MemberGrant {
    pub node: NodeId,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Member {
    pub user: UserId,
    pub name: String,
    pub email: String,
    /// Effective role on the node.
    pub role: Role,
    /// The person's grants on the node and its ancestors, nearest first. The one on the node
    /// itself, if any, is what `members.updateRole` and `members.remove` change.
    pub grants: Vec<MemberGrant>,
    /// Acts as owner of the root.
    pub instance_admin: bool,
    pub two_factor_enrolled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MemberList {
    /// By role, highest first, then by name.
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MemberResult {
    pub member: Member,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MemberArgs {
    pub node: NodeId,
    pub user: UserId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MembersUpdateRoleArgs {
    pub node: NodeId,
    pub user: UserId,
    pub role: Role,
    /// The `members:<id>` revision the change is based on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvitationState {
    Pending,
    Used,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Invitation {
    pub id: InvitationId,
    pub node: NodeId,
    pub role: Role,
    /// Who it is for. A signed-out invitee's new account gets this email.
    pub email: String,
    /// The first characters of the code.
    pub prefix: String,
    pub state: InvitationState,
    pub created_by: UserId,
    pub created_at: i64,
    pub expires_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub used_by: Option<UserId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MembersInviteArgs {
    pub node: NodeId,
    pub role: Role,
    pub email: String,
    /// 1 to 30. Defaults to 7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub lifetime_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationCreated {
    pub invitation: Invitation,
    /// `null` in a replayed answer.
    pub secret: Option<InvitationSecret>,
    /// True when this answer is a replay and the code was withheld.
    pub secret_withheld: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationSecret {
    pub code: String,
    /// `<public URL>/join?code=<code>`, or the path alone when no public URL is set.
    pub link: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationArgs {
    pub invitation: InvitationId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationResult {
    pub invitation: Invitation,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationList {
    pub invitations: Vec<Invitation>,
}
