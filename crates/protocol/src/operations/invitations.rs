//! Redeeming an invitation from its link, signed in or as a new account.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Role;
use crate::ids::NodeId;
use crate::operations::auth::Account;

crate::domain! {
    pub trait InvitationsHandlers;
    commands {
        /// Redeems an invitation. A signed-in caller gets the grant; a signed-out caller passes
        /// `account` and gets a new account with the invited email and a session. Unknown,
        /// used, revoked and expired codes answer the same `not_found`. An email that already
        /// has an account is `invalid_value` on `account`: sign in first. A caller who already
        /// holds a grant on the node keeps it and the code stays unused.
        "invitations.accept" => invitations_accept (Anonymous): InvitationsAcceptArgs => InvitationAccepted;
    }
    queries {
        /// What an invitation code offers, for the join page. Unknown, used, revoked and
        /// expired codes answer `not_found`.
        "invitations.preview" => invitations_preview (Anonymous): InvitationCodeArgs => InvitationPreview;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationCodeArgs {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationPreview {
    pub node_name: String,
    pub role: Role,
    pub email: String,
    pub inviter_name: String,
    pub expires_at: i64,
    /// An account with the invited email exists, so the invitee signs in rather than signing up.
    pub account_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct NewAccount {
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// 12 to 256 characters.
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationsAcceptArgs {
    pub code: String,
    /// Required when signed out, refused when signed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<NewAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct InvitationAccepted {
    pub node: NodeId,
    pub account: Account,
    /// The new session may only enroll a second factor until the person sets one up.
    pub enrollment_required: bool,
}
