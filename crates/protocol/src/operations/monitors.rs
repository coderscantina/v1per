//! Monitors on the node tree (milestone 05, monitoring contract). Every operation resolves the
//! caller's scope on the monitor's node: a monitor the caller cannot see answers `not_found`
//! like one that does not exist. Reads need `node.read`, changes `monitors.manage`.
//!
//! Changes and status changes are recorded on the node's `monitors:<id>` topic as `monitor.*`
//! events, and on the `monitor-tree:<id>` topic of the node and every ancestor. Each event
//! names its monitor. Subscribers are woken for status changes once the beat behind them is
//! readable from heartbeat storage, at most 5 seconds later; a stream that connects within that
//! moment may read the event first. Uptime, latency, bars and
//! heartbeats come from heartbeat storage; follow them with a normal query cache rather than
//! refetching on every event.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{ChannelId, MonitorId, NodeId, ProbeId};

crate::domain! {
    pub trait MonitorsHandlers;
    commands {
        /// Creates an active monitor on `node`. Needs `monitors.manage`. Secret fields need a
        /// `value`. A push monitor without `push_token` gets a random 32-character token.
        /// `group` must be a group monitor on the node or an ancestor the caller can read.
        /// `probes` must be probes `probes.available` offers on the node, else `invalid_value`
        /// on `probes`; the same holds for `channels` and `channels.available`. Without
        /// `channels` it uses the default channels on the node and its ancestors.
        "monitors.create" => monitors_create (Person, key): MonitorsCreateArgs => MonitorResult;
        /// Replaces a monitor's settings. Needs `monitors.manage`. A secret field without
        /// `value` keeps its stored value. The type may change; status history stays. Without
        /// `channels` its channels stay as they are.
        "monitors.update" => monitors_update (Person, key): MonitorsUpdateArgs => MonitorResult;
        /// Stops checking a monitor. Its status stays as it was. Needs `monitors.manage`.
        "monitors.pause" => monitors_pause (Person, key): MonitorArgs => MonitorResult;
        /// Starts checking a paused monitor again, at once. A monitor of an unsupported type
        /// cannot resume (`invalid_value` on `monitor`). Needs `monitors.manage`.
        "monitors.resume" => monitors_resume (Person, key): MonitorArgs => MonitorResult;
        /// Sets or clears the status of a manual monitor, recorded at once and then every
        /// interval. Other types answer `invalid_value` on `monitor`. Needs `monitors.manage`.
        "monitors.setManualStatus" => monitors_set_manual_status (Person, key): MonitorsSetManualStatusArgs => MonitorResult;
        /// Deletes a monitor with its state. Members of a deleted group leave the group.
        /// Recorded heartbeats stay until retention removes them. Needs `monitors.manage`.
        "monitors.delete" => monitors_delete (Person, key): MonitorArgs => MonitorDeleted;
    }
    queries {
        /// The monitors on one node, or with `subtree` on the node and every node below it,
        /// with their current status, last latency and 24-hour uptime, by name. Needs
        /// `node.read`. On the `monitors:<id>` topic, with `subtree` on `monitor-tree:<id>`.
        "monitors.list" => monitors_list (Person, key): MonitorsListArgs => MonitorList;
        /// One monitor with its settings, current state and the state on each probe. Secrets
        /// show only whether they are set; the push token only to callers with
        /// `monitors.manage`. On the `monitors:<id>` topic.
        "monitors.get" => monitors_get (Person, key): MonitorArgs => MonitorDetail;
        /// Recorded heartbeats, newest first, from every probe or from `probe` only. Pass
        /// `next_cursor` of a page as `cursor` for the next one.
        "monitors.heartbeats" => monitors_heartbeats (Person, key): HeartbeatsArgs => HeartbeatPage;
        /// Uptime over 24 hours, 7 days, 30 days and 1 year, counted like Uptime Kuma: of the
        /// monitor, or of one `probe`'s own results.
        "monitors.uptime" => monitors_uptime (Person, key): UptimeArgs => MonitorUptime;
        /// Latency of up beats per bucket in `[from, to)`, at most 1,500 buckets: of the
        /// monitor, or of one `probe`'s own results.
        "monitors.latency" => monitors_latency (Person, key): LatencyArgs => LatencySeries;
        /// Daily bars for the last `days` days in `timezone`, today included: of the monitor,
        /// or of one `probe`'s own results.
        "monitors.dayBars" => monitors_day_bars (Person, key): DayBarsArgs => DayBarList;
    }
}

/// Kuma's heartbeat statuses (monitoring contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatStatus {
    Down,
    Up,
    Pending,
    Maintenance,
}

/// What an operator sets on a manual monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ManualStatus {
    Up,
    Down,
    Pending,
}

/// When a monitor checked from several probes is down. `majority`: at least half of the live
/// probes report down. `any`: one does. `all`: every one does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Quorum {
    #[default]
    Majority,
    Any,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MonitorType {
    Http,
    Keyword,
    JsonQuery,
    Port,
    Ping,
    Dns,
    Push,
    Group,
    Manual,
    /// An imported Kuma type v1per does not run. Always paused.
    Unsupported,
}

/// A sealed credential. Send `value` to set or replace it, `clear: true` to remove it so the
/// check sends an empty value, or neither to keep the stored one. Answers never carry the
/// value, only whether one is set.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SecretField {
    #[serde(default, skip_serializing)]
    #[ts(optional)]
    pub value: Option<String>,
    /// Removes the stored value. Requests only; not together with `value`.
    #[serde(default, skip_serializing)]
    #[ts(as = "Option<bool>", optional)]
    pub clear: bool,
    /// A value is stored. Ignored in requests.
    #[serde(default)]
    pub set: bool,
}

/// The check and its options, tagged by Kuma's monitor type. Field names follow Kuma's
/// `monitor` columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum MonitorSettings {
    Http(HttpSettings),
    Keyword(KeywordSettings),
    JsonQuery(JsonQuerySettings),
    Port(PortSettings),
    Ping(PingSettings),
    Dns(DnsSettings),
    /// Records what `ALL /api/push/<token>` receives. Down when no push arrives within the
    /// interval plus one second.
    Push(PushSettings),
    /// Down when a member monitor is down, pending when one is pending or has no status yet.
    Group,
    /// Takes the status an operator sets with `monitors.setManualStatus`.
    Manual,
    /// Created only by the Kuma import; never accepted by `monitors.create` or `update`.
    Unsupported(UnsupportedSettings),
}

/// Request options of the http, keyword and json-query types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct HttpSettings {
    /// An http or https URL.
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub headers: Vec<HttpHeader>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    #[serde(default)]
    pub http_body_encoding: HttpBodyEncoding,
    #[serde(default)]
    pub auth: HttpAuth,
    /// Codes (`"200"`) or inclusive ranges (`"200-299"`) that pass.
    #[serde(default = "default_accepted_statuscodes")]
    pub accepted_statuscodes: Vec<String>,
    #[serde(default = "default_maxredirects")]
    pub maxredirects: u32,
    #[serde(default)]
    pub ignore_tls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ip_family: Option<IpFamily>,
    #[serde(default)]
    pub cache_bust: bool,
    /// Alerts the monitor's channels when a certificate of the chain is about to expire, at
    /// the instance's `tls_expiry_notify_days`. Not with `ignore_tls`.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub expiry_notification: bool,
}

fn default_method() -> String {
    "GET".to_owned()
}

fn default_accepted_statuscodes() -> Vec<String> {
    vec!["200-299".to_owned()]
}

fn default_maxredirects() -> u32 {
    10
}

/// A request header. A plain header carries `value`; a secret one, such as an API key,
/// carries `secret` and is sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct HttpHeader {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub secret: Option<SecretField>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HttpBodyEncoding {
    #[default]
    Json,
    Form,
    Xml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum IpFamily {
    Ipv4,
    Ipv6,
}

/// Kuma's `auth_method` with the fields each method uses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "auth_method", rename_all = "kebab-case")]
pub enum HttpAuth {
    #[default]
    None,
    Basic {
        basic_auth_user: String,
        basic_auth_pass: SecretField,
    },
    Bearer {
        bearer_token: SecretField,
    },
    /// OAuth 2 client credentials: a token from `oauth_token_url` is sent as a bearer token.
    #[serde(rename = "oauth2-cc")]
    OAuth2ClientCredentials {
        oauth_token_url: String,
        oauth_client_id: String,
        oauth_client_secret: SecretField,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        oauth_scopes: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        oauth_audience: Option<String>,
        #[serde(default)]
        oauth_auth_method: OAuthClientAuth,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OAuthClientAuth {
    #[default]
    ClientSecretBasic,
    ClientSecretPost,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct KeywordSettings {
    #[serde(flatten)]
    pub request: HttpSettings,
    pub keyword: String,
    /// Up when the keyword is missing.
    #[serde(default)]
    pub invert_keyword: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct JsonQuerySettings {
    #[serde(flatten)]
    pub request: HttpSettings,
    /// A JSONata expression; empty compares the whole response.
    #[serde(default)]
    pub json_path: String,
    #[serde(default)]
    pub json_path_operator: JsonPathOperator,
    pub expected_value: String,
    /// A failed comparison is down at once; only request failures retry.
    #[serde(default)]
    pub retry_only_on_status_code_failure: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub enum JsonPathOperator {
    #[default]
    #[serde(rename = "==")]
    Equals,
    #[serde(rename = "!=")]
    NotEquals,
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    LessOrEqual,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    GreaterOrEqual,
    #[serde(rename = "contains")]
    Contains,
}

/// A TCP connect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PortSettings {
    pub hostname: String,
    pub port: u16,
}

/// ICMP echo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PingSettings {
    pub hostname: String,
    /// 1 to 100.
    #[serde(default = "default_ping_count")]
    pub ping_count: u16,
    /// 1 to 65,500 bytes.
    #[serde(default = "default_packet_size")]
    pub packet_size: u16,
    /// Seconds to wait for each reply, above 0 and at most 60.
    #[serde(default = "default_ping_per_request_timeout")]
    pub ping_per_request_timeout: f64,
}

fn default_ping_count() -> u16 {
    1
}

fn default_packet_size() -> u16 {
    56
}

fn default_ping_per_request_timeout() -> f64 {
    2.0
}

/// A DNS lookup; up when it returns records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct DnsSettings {
    /// The name to resolve, or the IP address for PTR.
    pub hostname: String,
    pub dns_resolve_type: DnsRecordType,
    /// Comma-separated resolver addresses.
    #[serde(default = "default_dns_resolve_server")]
    pub dns_resolve_server: String,
    /// Resolver port.
    #[serde(default = "default_dns_port")]
    pub port: u16,
}

fn default_dns_resolve_server() -> String {
    "1.1.1.1".to_owned()
}

fn default_dns_port() -> u16 {
    53
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "UPPERCASE")]
pub enum DnsRecordType {
    A,
    Aaaa,
    Cname,
    Mx,
    Ns,
    Txt,
    Srv,
    Ptr,
    Caa,
    Soa,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PushSettings {
    /// The token in the push URL. In requests: a new token of at least 10 characters, the
    /// current one, or left out to keep the stored one. A shorter or taken token is
    /// `invalid_value`, as is one with `/`, `?`, `#`, `%` or a space. In answers: shown only to
    /// callers with `monitors.manage`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub push_token: Option<String>,
}

/// An imported Kuma monitor of a type v1per does not run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct UnsupportedSettings {
    /// Kuma's `type` value, such as `docker`.
    pub kuma_type: String,
    pub reason: String,
}

/// The settings a person edits. Intervals are seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorInput {
    /// 1 to 100 characters after trimming.
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub settings: MonitorSettings,
    /// Seconds between checks, 20 to 2,073,600.
    pub interval: u32,
    /// Seconds between checks while pending. Defaults to `interval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retry_interval: Option<u32>,
    /// Failed checks reported as pending before the monitor is down.
    #[serde(default)]
    pub max_retries: u32,
    /// Down beats between repeated down alerts; 0 sends one alert.
    #[serde(default)]
    pub resend_interval: u32,
    /// Seconds a check may take, above 0 and at most `interval`. Defaults to 80 % of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timeout: Option<f64>,
    /// Up when the check fails, down when it passes.
    #[serde(default)]
    pub upside_down: bool,
    #[serde(default)]
    pub quorum: Quorum,
    #[serde(default)]
    pub tags: Vec<String>,
    /// The group monitor this monitor belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group: Option<MonitorId>,
    /// The probes that check it, at most 20; empty means the built-in probe. Push, group and
    /// manual monitors run on the server and take none.
    #[serde(default)]
    pub probes: Vec<ProbeId>,
    /// The notification channels it alerts, from its node and the node's ancestors. See the
    /// commands for what an absent list means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channels: Option<Vec<ChannelId>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorsCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub monitor: MonitorInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorsUpdateArgs {
    pub monitor: MonitorId,
    /// The monitor's `revision` the edit is based on; `stale_revision` with the current one
    /// otherwise. Status changes do not move it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub input: MonitorInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorArgs {
    pub monitor: MonitorId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorsSetManualStatusArgs {
    pub monitor: MonitorId,
    /// Leave out to clear: the monitor is pending until a status is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<ManualStatus>,
}

/// A monitor as stored. Intervals are seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Monitor {
    pub id: MonitorId,
    pub node: NodeId,
    pub name: String,
    pub description: String,
    pub settings: MonitorSettings,
    pub interval: u32,
    pub retry_interval: u32,
    pub max_retries: u32,
    pub resend_interval: u32,
    pub timeout: f64,
    pub upside_down: bool,
    pub quorum: Quorum,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group: Option<MonitorId>,
    /// The probes that check it; empty for push, group and manual monitors.
    pub probes: Vec<ProbeId>,
    /// The notification channels it alerts.
    pub channels: Vec<ChannelId>,
    /// False while paused. Unsupported monitors are always paused.
    pub active: bool,
    /// Rises with every change to the monitor, not with status changes. Pass it as
    /// `expected_revision` to `monitors.update`.
    pub revision: i64,
    /// Manual monitors only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub manual_status: Option<ManualStatus>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorResult {
    pub monitor: Monitor,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorDeleted {
    pub monitor: MonitorId,
}

/// The monitor-level status after retries and quorum.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorState {
    pub status: HeartbeatStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    /// Failed checks in a row; pending while below `max_retries`.
    pub retries: u32,
    /// Time of the latest beat.
    pub beat_at: i64,
    /// Since when the monitor has this status.
    pub changed_at: i64,
}

/// One probe's view of a monitor, after that probe's retries.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ProbeState {
    pub probe: ProbeId,
    pub name: String,
    /// Counts in the quorum: the probe is online and reported within three intervals.
    pub live: bool,
    pub status: HeartbeatStatus,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    pub retries: u32,
    /// When the probe ran the check, on the probe's clock.
    pub checked_at: i64,
    /// When the server received the result.
    pub received_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorDetail {
    pub monitor: Monitor,
    /// Absent before the first beat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<MonitorState>,
    /// The assigned probes that reported. Empty for push, group and manual monitors, which
    /// the server evaluates itself.
    pub probes: Vec<ProbeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorsListArgs {
    pub node: NodeId,
    /// The monitors of every node below too.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub subtree: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorSummary {
    pub id: MonitorId,
    /// The node the monitor sits on.
    pub node: NodeId,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: MonitorType,
    pub active: bool,
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub group: Option<MonitorId>,
    /// Absent before the first beat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status: Option<HeartbeatStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub beat_at: Option<i64>,
    /// From 0 to 1; absent without up or down beats in the window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_24h: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorList {
    pub monitors: Vec<MonitorSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeartbeatsArgs {
    pub monitor: MonitorId,
    /// Only this probe's heartbeats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
    /// `next_cursor` of the page before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cursor: Option<String>,
    /// 1 to 100, default 25.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Heartbeat {
    pub time: i64,
    /// Absent for push, group and manual monitors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
    pub status: HeartbeatStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub latency_ms: Option<u32>,
    pub message: String,
    pub retries: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub http_status: Option<u16>,
    /// The TLS certificate's expiry, Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cert_expiry: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct HeartbeatPage {
    pub heartbeats: Vec<Heartbeat>,
    /// `cursor` for the next page; absent on the last one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct UptimeArgs {
    pub monitor: MonitorId,
    /// One probe's own results instead of the monitor's status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
}

/// Each ratio is from 0 to 1, absent without up or down beats in its window.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct MonitorUptime {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_24h: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_7d: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_30d: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime_1y: Option<f64>,
}

/// Bucket width; buckets are aligned in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    Minute,
    Hour,
    Day,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LatencyArgs {
    pub monitor: MonitorId,
    /// Unix milliseconds, inclusive.
    pub from: i64,
    /// Unix milliseconds, exclusive.
    pub to: i64,
    pub resolution: Resolution,
    /// One probe's own results instead of the monitor's status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LatencyPoint {
    /// Bucket start, Unix milliseconds.
    pub bucket: i64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct LatencySeries {
    /// Buckets with up beats only.
    pub points: Vec<LatencyPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DayBarsArgs {
    pub monitor: MonitorId,
    /// One probe's own results instead of the monitor's status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub probe: Option<ProbeId>,
    /// 1 to 90, default 90.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub days: Option<u32>,
    /// An IANA zone such as `Europe/Vienna`; default `UTC`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DayBar {
    /// `YYYY-MM-DD` in the requested zone.
    pub day: String,
    pub up: u32,
    pub down: u32,
    pub pending: u32,
    pub maintenance: u32,
    /// From 0 to 1, pending counted as down; absent without up or down beats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uptime: Option<f64>,
    /// Taken from an imported Kuma day, which is the UTC day of that date.
    pub utc_approximate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DayBarList {
    pub timezone: String,
    /// Oldest first.
    pub bars: Vec<DayBar>,
}
