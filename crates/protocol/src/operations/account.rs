//! The signed-in person's own account, grants and display preferences. API keys never reach
//! these operations.

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
        /// Replaces the caller's preferences. Absent fields fall back to the app's default.
        /// Records `account.preferences_changed` on the caller's `user:<id>` topic.
        "account.updatePreferences" => account_update_preferences (Person): Preferences => Preferences;
    }
    queries {
        /// The caller's account. Reachable from an enrollment-only session.
        "account.get" => account_get (Person, enrollment): Empty => AccountResult;
        /// The caller's own grants, by node name. Grants inherited from these are not listed.
        "account.grants" => account_grants (Person): Empty => GrantList;
        /// The caller's preferences, so another browser picks them up. On the caller's
        /// `user:<id>` topic.
        "account.preferences" => account_preferences (Person): Empty => Preferences;
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

/// Per-person display settings. The browser applies them; the server only stores them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Preferences {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub accent: Option<AppAccent>,
}

/// Accent colour presets of the admin app, apart from the status page presets. Indigo is the
/// default; the others are neon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AppAccent {
    Indigo,
    Ultraviolet,
    Magenta,
    Pink,
    Tangerine,
    Volt,
    Aqua,
    Azure,
}
