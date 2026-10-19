//! The public status page surface (milestone 09, `docs/contracts/status-page-api.md`). Pages are
//! served at `/s/{slug}` and on claimed hostnames, outside the operation registry: the HTML
//! carries a [`PageBoot`], `snapshot.json` answers a [`PageSnapshot`], `incidents.json` pages
//! through the incident history as [`PageIncidents`], and `events` streams the
//! same [`LiveFrame`](crate::live::LiveFrame)s as `/api/subscribe` on the page's topic.
//!
//! Every type here is built per audience. None carries a node, a monitor or static component
//! ID, a monitor's target, a person, a probe of a public page, or a draft incident.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{IncidentId, PageComponentId, PageGroupId};
use crate::operations::incidents::{Impact, IncidentState};
use crate::operations::monitors::{HeartbeatStatus, LatencyPoint};

/// The color of a component, a day or the whole page, from best to worst. Maintenance ranks
/// above operational and below every outage (monitoring contract, Component color and page
/// status).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    Operational,
    Maintenance,
    Degraded,
    PartialOutage,
    MajorOutage,
}

/// Who a snapshot is built for. Grant holders on the page's node get `internal`; everyone
/// else the page admits gets `public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageAudience {
    Public,
    Internal,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Locale {
    #[default]
    En,
    De,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::En, Locale::De];

    pub fn as_str(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::De => "de",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|locale| locale.as_str() == value)
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PageTheme {
    /// Follows the visitor's system appearance.
    #[default]
    Auto,
    Light,
    Dark,
}

/// A Kit accent preset, applied as `data-accent` on the page.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Accent {
    #[default]
    Indigo,
    Blue,
    Violet,
    Rose,
    Orange,
    Amber,
    Emerald,
    Cyan,
}

/// What the HTML of `/s/{slug}` (or a claimed hostname's root) carries as JSON in
/// `<script id="v1per-page" type="application/json">`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PageBoot {
    /// The visitor may read the page.
    Page {
        /// Path prefix of the page's other routes: `/s/<slug>`, or empty on a claimed hostname.
        base: String,
        snapshot: Box<PageSnapshot>,
    },
    /// The page is restricted and the visitor has no access yet.
    Gate { base: String, gate: PageGate },
}

/// Title, look and languages, shared by the page and its sign-in gate.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageAppearance {
    pub title: String,
    pub theme: PageTheme,
    pub accent: Accent,
    /// Path of the logo below the page's `base`, such as `/logo?v=1760000000000`; absent
    /// without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub logo: Option<String>,
    pub default_locale: Locale,
    /// Enabled locales, the default first.
    pub locales: Vec<Locale>,
}

/// How a restricted page can be opened.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageGate {
    pub appearance: PageAppearance,
    /// The locale the request asked for among the enabled ones.
    pub locale: Locale,
    /// `POST <base>/access/password` works.
    pub password: bool,
    /// `POST <base>/access/email` sends sign-in links to allowed addresses.
    pub email: bool,
    /// Where grant holders sign in to the instance and come back to the page. Absent on a
    /// claimed hostname when the instance has no public URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sign_in_url: Option<String>,
}

/// One page as one audience sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSnapshot {
    pub appearance: PageAppearance,
    pub description: String,
    pub footer: String,
    /// IANA zone the daily bars are counted in.
    pub timezone: String,
    /// The locale incident titles and server-written texts are rendered in.
    pub locale: Locale,
    pub audience: PageAudience,
    /// The worst status of the visible components.
    pub status: PageStatus,
    pub groups: Vec<PublicGroup>,
    /// Published incidents: open ones and those resolved within the bars' 90 days, newest
    /// first.
    pub incidents: Vec<PublicIncident>,
    /// Maintenance under way or starting within 14 days.
    pub maintenance: Vec<PublicMaintenance>,
    /// `page:<id>@<seq>`: subscribe to `<base>/events` from here to follow the snapshot.
    pub cursor: String,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicGroup {
    pub id: PageGroupId,
    /// Empty for a group shown without a heading.
    pub name: String,
    pub default_open: bool,
    pub components: Vec<PublicComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicComponent {
    pub id: PageComponentId,
    pub name: String,
    /// A static component's description; empty for monitors.
    pub description: String,
    /// Absent while a monitor has no status, for example when paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<PageStatus>,
    /// Over the 90 bars; absent without up or down heartbeats and for static components.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_90d: Option<f64>,
    /// 90 local days in `timezone`, oldest first, today last.
    pub bars: Vec<PageBar>,
    /// Response times and per-probe status, for the `internal` audience only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<ComponentDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageBar {
    /// `YYYY-MM-DD` in the page timezone.
    pub day: String,
    /// Absent for a day without data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<PageStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime: Option<f64>,
    /// Counted from an imported Kuma daily row: the UTC day of the same date.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub utc_approximate: bool,
    /// Heartbeats that day were under maintenance.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub maintenance: bool,
    /// Published incidents that touched the component that day.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<IncidentId>>", optional)]
    pub incidents: Vec<IncidentId>,
}

/// What grant holders see beyond the public snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentDetail {
    /// Hourly response times over the last 24 hours; per minute over the last 2 hours while
    /// there are fewer than two hourly points, as for a new monitor.
    pub latency: Vec<LatencyPoint>,
    pub probes: Vec<ProbeView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeView {
    pub name: String,
    pub status: HeartbeatStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    pub checked_at: i64,
}

/// One page of `<base>/incidents.json`: published incidents naming a visible component,
/// newest first, without the snapshot's 90-day bound.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageIncidents {
    pub incidents: Vec<PublicIncident>,
    /// Pass as `cursor` for the next older page; absent on the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicIncident {
    pub id: IncidentId,
    pub title: String,
    pub state: IncidentState,
    /// The worst current impact among the components shown; absent while none has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub impact: Option<Impact>,
    pub started_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved_at: Option<i64>,
    /// The page's components it affects.
    pub components: Vec<PageComponentId>,
    /// Newest first.
    pub updates: Vec<PublicIncidentUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicIncidentUpdate {
    pub state: IncidentState,
    pub message: String,
    pub created_at: i64,
    pub impacts: Vec<PublicImpact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicImpact {
    pub component: PageComponentId,
    pub impact: Impact,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PublicMaintenance {
    pub title: String,
    pub description: String,
    /// Under way now.
    pub active: bool,
    /// Start of the current or next window; absent when it has none, as for manual ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub starts_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ends_at: Option<i64>,
    pub components: Vec<PageComponentId>,
}

/// `POST <base>/access/password`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PagePasswordBody {
    pub password: String,
}

/// `POST <base>/access/email`: sends a sign-in link if the address is allowed. The answer is
/// the same either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageEmailBody {
    pub email: String,
    /// Language of the email; absent: the page's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub locale: Option<Locale>,
}

/// `POST <base>/access/email/redeem` with the `code` of the emailed link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageRedeemBody {
    pub code: String,
}
