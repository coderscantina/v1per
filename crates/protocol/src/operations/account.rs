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
    /// How the monitor page draws the status block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status_card: Option<StatusCardStyle>,
    /// How numbers group and mark decimals; absent follows the app language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub number_format: Option<NumberFormat>,
    /// The order of a date's parts, written in digits; absent follows the app language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub date_format: Option<DateFormat>,
    /// Absent follows the app language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub time_format: Option<TimeFormat>,
    /// The zone times are shown in; absent is the browser's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub time_zone: Option<DisplayZone>,
    /// The first day of a week in lists of weekdays; absent follows the app language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub week_start: Option<WeekStart>,
}

/// Digit grouping and decimal mark, named by the grouping mark, then the decimal mark where
/// one grouping mark goes with either.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NumberFormat {
    /// 1,234.5
    Comma,
    /// 1.234,5
    Period,
    /// 1 234,5
    Space,
    /// 1 234.5
    SpacePeriod,
    /// 1'234.5
    Apostrophe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    /// 20.10.2026
    DayMonthYear,
    /// 10/20/2026
    MonthDayYear,
    /// 2026-10-20
    YearMonthDay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    /// 14:30
    H24,
    /// 2:30 PM
    H12,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WeekStart {
    Monday,
    Saturday,
    Sunday,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DisplayZone {
    /// The browser's own zone.
    Local,
    Utc,
}

/// Designs of the status block on the monitor page. Banner is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusCardStyle {
    /// A tinted block with the status, its message and the latest check.
    Banner,
    /// The status in large type with uptime figures beside it.
    Hero,
    /// One line.
    Compact,
    /// A ring of the 30 day uptime around the status.
    Gauge,
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
