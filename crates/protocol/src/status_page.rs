//! The public status page surface (milestone 09, `docs/contracts/status-page-api.md`). Pages are
//! served at `/s/{slug}` and on claimed hostnames, outside the operation registry: the HTML
//! carries a [`PageBoot`], `snapshot.json` answers a [`PageSnapshot`], `incidents.json` pages
//! through the incident history as [`PageIncidents`], `response-times.json` answers a
//! [`PageResponseTimes`], and `events` streams the
//! same [`LiveFrame`](crate::live::LiveFrame)s as `/api/subscribe` on the page's topic.
//!
//! Every type here is built per audience. None carries a node, a monitor or static component
//! ID, a monitor's target, a person, a probe of a public page, a draft incident, or an SLA's
//! ID, monitors, channels or incidents.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{IncidentId, PageComponentId, PageGroupId};
use crate::operations::incidents::{Impact, IncidentState, Postmortem};
use crate::operations::monitors::{HeartbeatStatus, LatencyPoint};
use crate::operations::slas::{SlaCombine, SlaOutcome, SlaPeriodKind};

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
    /// The SLAs the page shows, in the page's order.
    pub slas: Vec<PageSla>,
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
    /// Monitors only: response times and per-probe status for the `internal` audience, and
    /// response times alone for the `public` one on a page with `response_times` set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<ComponentDetail>,
    /// The checks behind `uptime_90d`: the sums of the bars' counts.
    #[serde(flatten)]
    pub counts: PageCounts,
}

/// The monitor-level checks behind an uptime figure. Uptime is `up / (up + down)`; checks
/// under maintenance are left out of it. Each count is absent while zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageCounts {
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(as = "Option<u32>", optional)]
    pub up: u32,
    /// Pending checks, still retrying, count as down.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(as = "Option<u32>", optional)]
    pub down: u32,
    /// Checks under maintenance.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[ts(as = "Option<u32>", optional)]
    pub maint: u32,
}

fn is_zero(count: &u32) -> bool {
    *count == 0
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
    /// The checks behind `uptime`.
    #[serde(flatten)]
    pub counts: PageCounts,
}

/// What a monitor shows beyond its bars.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ComponentDetail {
    /// Hourly response times over the last 24 hours; per minute over the last 2 hours while
    /// there are fewer than two hourly points, as for a new monitor.
    pub latency: Vec<LatencyPoint>,
    /// Each assigned probe's last check. Always empty for the `public` audience.
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

/// The span `<base>/response-times.json` covers, ending now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
pub enum ResponseRange {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
    #[serde(rename = "90d")]
    Quarter,
}

impl ResponseRange {
    pub const ALL: [ResponseRange; 4] = [Self::Day, Self::Week, Self::Month, Self::Quarter];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "24h",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "90d",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|range| range.as_str() == value)
    }
}

/// `<base>/response-times.json`: one monitor component's response times over a range, with
/// the incidents and maintenance that touched it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageResponseTimes {
    pub range: ResponseRange,
    /// The span the points cover, Unix milliseconds. A monitor with fewer than two hourly
    /// points gets its last 2 hours per minute over `24h`, as the snapshot does.
    pub from: i64,
    pub to: i64,
    /// Bucket width of the points in milliseconds; a longer step between two points is a gap.
    pub step: i64,
    /// Every check of the monitor.
    pub overall: ResponseSeries,
    /// Per probe for the `internal` audience, per probe location for the `public` one on a
    /// page with `probe_locations`; empty otherwise.
    pub series: Vec<ResponseSeries>,
    /// Oldest first.
    pub marks: Vec<ResponseMark>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ResponseSeries {
    /// The probe's name, with its location when set, or "City, CC"; absent on `overall`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    /// ISO 3166-1 alpha-2, when the series has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub country: Option<String>,
    /// Buckets with up beats, oldest first.
    pub points: Vec<LatencyPoint>,
    /// Over the up checks in the range, in milliseconds; absent without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub avg_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub p50_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub p95_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub p99_ms: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMarkKind {
    Incident,
    Maintenance,
}

/// A published incident that touched the component, or a maintenance window that covered it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ResponseMark {
    pub kind: ResponseMarkKind,
    /// Incidents only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub incident: Option<IncidentId>,
    /// An incident's worst impact on the component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub impact: Option<Impact>,
    pub title: String,
    pub from: i64,
    /// Absent while it lasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub to: Option<i64>,
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
    /// Plain text the team wrote after resolution; absent without one and while the incident
    /// is open again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub postmortem: Option<Postmortem>,
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

/// An SLA as a page shows it: targets and aggregates per period, never an incident or a
/// monitor the page does not show.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSla {
    pub name: String,
    /// IANA zone the periods are counted in.
    pub timezone: String,
    pub period: SlaPeriodKind,
    pub combine: SlaCombine,
    /// The running period, as of the snapshot; absent before the contract starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub current: Option<PageSlaPeriod>,
    /// Up to 12 closed periods, newest first. A period is listed once frozen, an hour after
    /// it closes.
    pub history: Vec<PageSlaPeriod>,
}

/// One period against the targets it was computed with.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaPeriod {
    /// `YYYY-MM-DD`, the first day in the SLA's timezone.
    pub start: String,
    /// First instant of the period.
    pub starts_at: i64,
    /// First instant after the period.
    pub ends_at: i64,
    /// The worst commitment: missed, at risk, on track or met. No data when none has data.
    pub outcome: SlaOutcome,
    /// Absent without an availability target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability: Option<PageSlaAvailability>,
    /// Absent without a latency target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency: Option<PageSlaLatency>,
    /// Absent without a response target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub response: Option<PageSlaClock>,
    /// Absent without a resolution target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolution: Option<PageSlaClock>,
    /// Per covered monitor the page shows, by its page component. `internal` audience only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<PageSlaMonitor>>", optional)]
    pub monitors: Vec<PageSlaMonitor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaAvailability {
    /// In percent.
    pub target: f64,
    pub outcome: SlaOutcome,
    /// In percent: the service in `any_down` mode, the lowest monitor in `each` mode. Absent:
    /// no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub actual: Option<f64>,
    /// Share of the downtime budget used, 1 being all of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub budget_used: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaLatency {
    /// Average over up beats, in milliseconds.
    pub target_ms: u32,
    pub outcome: SlaOutcome,
    /// The highest monitor average, in milliseconds. Absent: no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub actual_ms: Option<f64>,
}

/// Response or resolution times of the incidents that started in the period.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaClock {
    pub targets: PageSlaTargets,
    pub outcome: SlaOutcome,
    /// Incidents that met or missed the target.
    pub counted: u32,
    pub met: u32,
    /// `met / counted`; absent while none is counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub within_target: Option<f64>,
    /// Average business milliseconds of the counted incidents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub average_ms: Option<i64>,
}

/// Business minutes per worst impact; absent: no target for that impact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaTargets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub major_outage: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub partial_outage: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub degraded: Option<u32>,
}

/// One covered monitor, named by the page component that shows it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct PageSlaMonitor {
    pub component: PageComponentId,
    /// In percent. Absent: no data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability: Option<f64>,
    /// In `each` mode against the availability target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub availability_outcome: Option<SlaOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_outcome: Option<SlaOutcome>,
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
