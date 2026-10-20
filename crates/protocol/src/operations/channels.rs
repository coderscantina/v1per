//! Notification channels on the node tree (decision 0005, milestone 07). A channel lives on a
//! node and is usable by monitors on that node and below. Its settings follow Uptime Kuma
//! 2.5.0's notification form for the provider; credentials are sealed and answers only say
//! whether one is set. Managing channels and reading their settings and delivery log needs
//! `channels.manage`; picking them for a monitor needs `node.read`.
//!
//! Events go to the channel node's `channels:<id>` topic: `channel.created`, `channel.updated`
//! and `channel.deleted` with the channel's ID, and `channel.delivery` with the delivery's ID
//! (and its channel, unless it is a test of an unsaved form) whenever a delivery is queued,
//! attempted or finished.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{ChannelId, DeliveryId, MonitorId, NodeId};
use crate::operations::monitors::SecretField;
use crate::operations::nodes::NodeArgs;

crate::domain! {
    pub trait ChannelsHandlers;
    commands {
        /// Creates a channel on `node`. Needs `channels.manage`. With `apply_existing`, every
        /// monitor on the node and below uses it, like Kuma's "apply on all existing monitors".
        "channels.create" => channels_create (Person): ChannelsCreateArgs => ChannelResult;
        /// Replaces a channel's name, flags and settings. A secret field without `value` keeps
        /// its stored value. Needs `channels.manage` on the channel's node.
        "channels.update" => channels_update (Person): ChannelsUpdateArgs => ChannelResult;
        /// Deletes a channel with its monitor links and delivery log. Needs `channels.manage`.
        "channels.delete" => channels_delete (Person): ChannelArgs => ChannelDeleted;
        /// Queues a test, Kuma's "Test" button, through the outbox with a single attempt. It
        /// sends Kuma's test message, or with `sample` a monitor down or up alert rendered like
        /// a real one, for checking templates. `settings` is the form as it stands; secret fields without `value` come
        /// from `channel`, which must be on `node`. Follow `channel.delivery` on the node's
        /// `channels:<id>` topic and read the outcome with `channels.delivery`. Needs
        /// `channels.manage`.
        "channels.test" => channels_test (Person): ChannelsTestArgs => ChannelTestQueued;
    }
    queries {
        /// The channels on one node with their settings, by name. Needs `channels.manage`. On
        /// the `channels:<id>` topic.
        "channels.list" => channels_list (Person): NodeArgs => ChannelList;
        /// One channel with its settings. Needs `channels.manage` on its node. On the node's
        /// `channels:<id>` topic.
        "channels.get" => channels_get (Person): ChannelArgs => ChannelResult;
        /// The channels a monitor on `node` may use: those on the node and its ancestors,
        /// nearest node first, by name. Needs `node.read`. Channels on ancestors the caller
        /// cannot read are listed without their node. On the node's `channels:<id>` topic,
        /// which does not record changes on ancestors.
        "channels.available" => channels_available (Person, key): NodeArgs => ChannelChoiceList;
        /// A channel's deliveries, newest first. Needs `channels.manage` on its node. Pass
        /// `next_cursor` of a page as `cursor` for the next one.
        "channels.deliveries" => channels_deliveries (Person): DeliveriesArgs => DeliveryPage;
        /// The deliveries about one monitor, newest first, through the channels on its node
        /// and on the ancestors the caller can read. Needs `node.read` on the monitor's node.
        /// `error` and `detail` are left out for channels on nodes where the caller lacks
        /// `channels.manage`. Paged like `channels.deliveries`. On the monitor node's
        /// `channels:<id>` topic; a delivery through an ancestor's channel is announced on
        /// that node's topic.
        "channels.monitorDeliveries" => channels_monitor_deliveries (Person): MonitorDeliveriesArgs => DeliveryPage;
        /// One delivery, such as a test. Needs `channels.manage` on its node.
        "channels.delivery" => channels_delivery (Person): DeliveryArgs => DeliveryResult;
    }
}

/// A provider's settings, with Kuma's field meanings under snake_case names. Tagged by Kuma's
/// `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelSettings {
    Smtp(SmtpChannel),
    Discord(DiscordChannel),
    Webhook(WebhookChannel),
    Slack(SlackChannel),
    Teams(TeamsChannel),
    Telegram(TelegramChannel),
    Ntfy(NtfyChannel),
    Pushover(PushoverChannel),
    /// An imported Kuma provider V1Per does not send to yet. Its settings stay sealed. Never
    /// accepted by `channels.create` or `update`.
    Unsupported(UnsupportedChannel),
}

/// Email. Without `host` the instance SMTP server sends it, and an absent `from` uses the
/// instance sender.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SmtpChannel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub port: Option<u16>,
    /// Implicit TLS. Off means STARTTLS when the server offers it.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub secure: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub ignore_tls_error: bool,
    /// Only with `secure` off: never upgrade to TLS.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub ignore_starttls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: SecretField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from: Option<String>,
    /// Comma-separated addresses. At least one of `to`, `cc` and `bcc` is needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub bcc: Option<String>,
    /// A Liquid template with Kuma's variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub custom_subject: Option<String>,
    /// A Liquid template with Kuma's variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub custom_body: Option<String>,
    /// Sends `custom_body` as HTML.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub html_body: bool,
    /// A JSON object of extra headers.
    #[serde(default)]
    pub additional_headers: SecretField,
    /// Signs with DKIM when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dkim_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dkim_key_selector: Option<String>,
    #[serde(default)]
    pub dkim_private_key: SecretField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dkim_hash_algo: Option<String>,
    /// Colon-separated header names to sign.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dkim_header_field_names: Option<String>,
    /// Colon-separated header names to leave unsigned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dkim_skip_fields: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct DiscordChannel {
    /// Required.
    #[serde(default)]
    pub webhook_url: SecretField,
    /// Default `Uptime Kuma`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prefix_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message_format: Option<DiscordMessageFormat>,
    /// Kuma's older switch for a custom template, read only without `message_format`.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub use_message_template: bool,
    /// A Liquid template with Kuma's variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel_type: Option<DiscordChannelType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub post_name: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub disable_url: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub suppress_notifications: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscordMessageFormat {
    Normal,
    Minimalist,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiscordChannelType {
    Channel,
    CreateNewForumPost,
    PostToThread,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct WebhookChannel {
    /// Required.
    #[serde(default)]
    pub url: SecretField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub method: Option<WebhookMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content_type: Option<WebhookContentType>,
    /// The body for `content_type: custom`, a Liquid template with Kuma's variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub custom_body: Option<String>,
    /// A JSON object of extra headers, often with a token.
    #[serde(default)]
    pub additional_headers: SecretField,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookMethod {
    Post,
    Get,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebhookContentType {
    Json,
    FormData,
    Custom,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct SlackChannel {
    /// Required.
    #[serde(default)]
    pub webhook_url: SecretField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon_emoji: Option<String>,
    /// Appends `<!channel>` to the message.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub channel_notify: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub rich_message: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub use_template: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub template: Option<String>,
    /// Absent counts as on, as in Kuma.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub include_group_name: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TeamsChannel {
    /// Required.
    #[serde(default)]
    pub webhook_url: SecretField,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub enable_tags: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct TelegramChannel {
    /// Required.
    #[serde(default)]
    pub bot_token: SecretField,
    /// Required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub chat_id: Option<String>,
    /// Default `https://api.telegram.org`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub server_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message_thread_id: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub send_silently: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub protect_content: bool,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub use_template: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parse_mode: Option<TelegramParseMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TelegramParseMode {
    Plain,
    Html,
    MarkdownV2,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct NtfyChannel {
    /// Default `https://ntfy.sh`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub server_url: Option<String>,
    /// Required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub topic: Option<String>,
    /// 1 to 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority: Option<u8>,
    /// 1 to 5, for down messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority_down: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub auth: Option<NtfyAuth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: SecretField,
    #[serde(default)]
    pub access_token: SecretField,
    /// Value of the `X-Call` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub call: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub icon: Option<String>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub use_template: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub custom_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub custom_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NtfyAuth {
    None,
    UsernamePassword,
    AccessToken,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct PushoverChannel {
    /// Required.
    #[serde(default)]
    pub user_key: SecretField,
    /// Required.
    #[serde(default)]
    pub app_token: SecretField,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sound: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sound_up: Option<String>,
    /// -2 to 2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub priority: Option<i8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub device: Option<String>,
    /// Seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ttl: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
pub struct UnsupportedChannel {
    /// Kuma's `type`, such as `PagerDuty`.
    pub kuma_type: String,
}

/// What a person edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelInput {
    /// 1 to 100 characters after trimming.
    pub name: String,
    /// New monitors on the node and below use it unless their creator picks channels.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub is_default: bool,
    /// An inactive channel queues nothing. Default on.
    #[serde(default = "active_default")]
    #[ts(as = "Option<bool>", optional)]
    pub active: bool,
    pub settings: ChannelSettings,
}

fn active_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelsCreateArgs {
    pub node: NodeId,
    #[serde(flatten)]
    pub channel: ChannelInput,
    /// Links every monitor on the node and below to the channel.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub apply_existing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelsUpdateArgs {
    pub channel: ChannelId,
    /// The channel's `revision` the edit is based on; `stale_revision` with the current one
    /// otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_revision: Option<i64>,
    #[serde(flatten)]
    pub input: ChannelInput,
    /// Links every monitor on the channel's node and below to it.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub apply_existing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelArgs {
    pub channel: ChannelId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelsTestArgs {
    pub node: NodeId,
    /// The saved channel the form edits, for its stored secrets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<ChannelId>,
    /// Used in the test message, `<name> Testing`.
    pub name: String,
    pub settings: ChannelSettings,
    /// Send a monitor alert instead of Kuma's test message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sample: Option<TestSample>,
    /// With `sample`: the monitor whose details fill the alert. Must be on `node` or below.
    /// Without it a made-up example monitor is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<MonitorId>,
}

/// The alert a channel test sends in place of Kuma's test message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestSample {
    MonitorDown,
    MonitorUp,
}

/// A channel as stored. Secrets show only whether they are set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
pub struct Channel {
    pub id: ChannelId,
    pub node: NodeId,
    pub name: String,
    pub is_default: bool,
    pub active: bool,
    /// Why an imported channel cannot send, such as an unsupported provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub inactive_reason: Option<String>,
    pub settings: ChannelSettings,
    /// Monitors that use it.
    pub monitors: u32,
    /// Rises with every change. Pass it as `expected_revision` to `channels.update`.
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelResult {
    pub channel: Channel,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelDeleted {
    pub channel: ChannelId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelList {
    pub channels: Vec<Channel>,
}

/// A channel a monitor may use.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelChoice {
    pub id: ChannelId,
    pub name: String,
    /// Kuma's provider type, such as `smtp` or `discord`.
    pub provider: String,
    pub is_default: bool,
    pub active: bool,
    /// On the node itself rather than on an ancestor.
    pub own: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelChoiceList {
    pub channels: Vec<ChannelChoice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryEvent {
    MonitorDown,
    MonitorUp,
    CertificateExpiry,
    ProbeOffline,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// Waiting for its first attempt or a retry.
    Pending,
    Delivered,
    /// The provider refused it for good, or retries ran out after 24 hours.
    Failed,
    /// Not sent: its monitor is under maintenance.
    Suppressed,
}

/// One message for one channel. Never holds a secret.
#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct Delivery {
    pub id: DeliveryId,
    /// Absent for a test of an unsaved form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub channel: Option<ChannelId>,
    pub event: DeliveryEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub monitor: Option<MonitorId>,
    /// Kuma's message line, such as `[API] [🔴 Down] timeout`.
    pub summary: String,
    pub state: DeliveryState,
    pub attempts: u32,
    /// When a pending delivery is tried next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_attempt_at: Option<i64>,
    /// Why the latest attempt failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub error: Option<String>,
    /// What the provider answered to the successful attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct ChannelTestQueued {
    pub delivery: DeliveryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DeliveriesArgs {
    pub channel: ChannelId,
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
pub struct MonitorDeliveriesArgs {
    pub monitor: MonitorId,
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
pub struct DeliveryPage {
    pub deliveries: Vec<Delivery>,
    /// `cursor` for the next page; absent on the last one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DeliveryArgs {
    pub delivery: DeliveryId,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, JsonSchema)]
pub struct DeliveryResult {
    pub delivery: Delivery,
}
