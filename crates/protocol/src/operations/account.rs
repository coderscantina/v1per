//! The signed-in person's own account and grants. API keys never reach these operations.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::access::Role;
use crate::envelope::Empty;
use crate::ids::NodeId;
use crate::operations::auth::Account;

crate::domain! {
    pub trait AccountHandlers;
    commands {
        /// Replaces the password after checking the current one, and ends every other session
        /// of the caller. A wrong current password is `invalid_credentials` and counts toward
        /// the login limit. The new password has 12 to 256 characters.
        "account.changePassword" => account_change_password (Person): ChangePasswordArgs => Empty;
    }
    queries {
        /// The caller's account. Reachable from an enrollment-only session.
        "account.get" => account_get (Person, enrollment): Empty => AccountResult;
        /// The caller's own grants, by node name. Grants inherited from these are not listed.
        "account.grants" => account_grants (Person): Empty => GrantList;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct AccountResult {
    pub account: Account,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChangePasswordArgs {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Grant {
    pub node: NodeId,
    pub name: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct GrantList {
    pub grants: Vec<Grant>,
}
