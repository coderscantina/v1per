//! Channel configs, deserialized straight from Kuma's `notification.config` JSON.
//!
//! Field names are Kuma's, unknown fields are ignored, and `type` picks the provider. Types
//! v1per does not send yet keep their raw JSON in [`UnsupportedConfig`].

use std::fmt;
use std::str::FromStr;

use serde::de::Error;
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};

use crate::de;
use crate::secret::Secret;

#[derive(Debug)]
pub enum ChannelConfig {
    Smtp(SmtpConfig),
    Discord(DiscordConfig),
    Webhook(WebhookConfig),
    Slack(SlackConfig),
    Teams(TeamsConfig),
    Telegram(TelegramConfig),
    Ntfy(NtfyConfig),
    Pushover(PushoverConfig),
    Unsupported(UnsupportedConfig),
}

impl ChannelConfig {
    /// Kuma's `config.type`.
    pub fn kind(&self) -> &str {
        match self {
            Self::Smtp(_) => "smtp",
            Self::Discord(_) => "discord",
            Self::Webhook(_) => "webhook",
            Self::Slack(_) => "slack",
            Self::Teams(_) => "teams",
            Self::Telegram(_) => "telegram",
            Self::Ntfy(_) => "ntfy",
            Self::Pushover(_) => "pushover",
            Self::Unsupported(config) => config.kind(),
        }
    }
}

impl<'de> Deserialize<'de> for ChannelConfig {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Map::<String, Value>::deserialize(deserializer)?;
        let kind = match raw.get("type") {
            Some(Value::String(kind)) => kind.clone(),
            _ => return Err(D::Error::custom("notification config has no string `type`")),
        };
        fn parse<T: serde::de::DeserializeOwned, E: Error>(
            raw: Map<String, Value>,
        ) -> Result<T, E> {
            serde_json::from_value(Value::Object(raw)).map_err(E::custom)
        }
        Ok(match kind.as_str() {
            "smtp" => Self::Smtp(parse(raw)?),
            "discord" => Self::Discord(parse(raw)?),
            "webhook" => Self::Webhook(parse(raw)?),
            "slack" => Self::Slack(parse(raw)?),
            "teams" => Self::Teams(parse(raw)?),
            "telegram" => Self::Telegram(parse(raw)?),
            "ntfy" => Self::Ntfy(parse(raw)?),
            "pushover" => Self::Pushover(parse(raw)?),
            _ => Self::Unsupported(UnsupportedConfig { kind, raw }),
        })
    }
}

/// A provider v1per cannot send to yet. Keeps the whole config, secrets included.
pub struct UnsupportedConfig {
    kind: String,
    raw: Map<String, Value>,
}

impl UnsupportedConfig {
    pub fn kind(&self) -> &str {
        &self.kind
    }

    pub fn raw(&self) -> &Map<String, Value> {
        &self.raw
    }
}

impl fmt::Debug for UnsupportedConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnsupportedConfig")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Kuma `smtp`: `server/notification-providers/smtp.js`.
#[derive(Debug, Default, Deserialize)]
pub struct SmtpConfig {
    #[serde(rename = "smtpHost", default, deserialize_with = "de::text")]
    pub host: Option<String>,
    #[serde(rename = "smtpPort", default, deserialize_with = "de::number")]
    pub port: Option<u16>,
    /// Implicit TLS. Off means STARTTLS when the server offers it.
    #[serde(rename = "smtpSecure", default, deserialize_with = "de::flag")]
    pub secure: bool,
    #[serde(rename = "smtpIgnoreTLSError", default, deserialize_with = "de::flag")]
    pub ignore_tls_error: bool,
    /// Only with `secure` off: never upgrade, send in plain text.
    #[serde(rename = "smtpIgnoreSTARTTLS", default, deserialize_with = "de::flag")]
    pub ignore_starttls: bool,
    #[serde(rename = "smtpUsername", default, deserialize_with = "de::text")]
    pub username: Option<String>,
    #[serde(rename = "smtpPassword", default, deserialize_with = "de::secret")]
    pub password: Option<Secret>,
    #[serde(rename = "smtpFrom", default, deserialize_with = "de::text")]
    pub from: Option<String>,
    /// Comma separated address lists, as in Kuma.
    #[serde(rename = "smtpTo", default, deserialize_with = "de::text")]
    pub to: Option<String>,
    #[serde(rename = "smtpCC", default, deserialize_with = "de::text")]
    pub cc: Option<String>,
    #[serde(rename = "smtpBCC", default, deserialize_with = "de::text")]
    pub bcc: Option<String>,
    #[serde(rename = "customSubject", default, deserialize_with = "de::text")]
    pub custom_subject: Option<String>,
    #[serde(rename = "customBody", default, deserialize_with = "de::text")]
    pub custom_body: Option<String>,
    /// Sends a custom body as HTML. Has no effect without one.
    #[serde(rename = "htmlBody", default, deserialize_with = "de::flag")]
    pub html_body: bool,
    /// A JSON object of extra headers.
    #[serde(
        rename = "smtpAdditionalHeaders",
        default,
        deserialize_with = "de::secret"
    )]
    pub additional_headers: Option<Secret>,
    /// DKIM signing is on only when this is set.
    #[serde(rename = "smtpDkimDomain", default, deserialize_with = "de::text")]
    pub dkim_domain: Option<String>,
    #[serde(rename = "smtpDkimKeySelector", default, deserialize_with = "de::text")]
    pub dkim_key_selector: Option<String>,
    #[serde(
        rename = "smtpDkimPrivateKey",
        default,
        deserialize_with = "de::secret"
    )]
    pub dkim_private_key: Option<Secret>,
    /// Kept for import; signing always uses SHA-256.
    #[serde(rename = "smtpDkimHashAlgo", default, deserialize_with = "de::text")]
    pub dkim_hash_algo: Option<String>,
    /// Colon separated header names to sign.
    #[serde(
        rename = "smtpDkimheaderFieldNames",
        default,
        deserialize_with = "de::text"
    )]
    pub dkim_header_field_names: Option<String>,
    /// Colon separated header names to leave unsigned.
    #[serde(rename = "smtpDkimskipFields", default, deserialize_with = "de::text")]
    pub dkim_skip_fields: Option<String>,
}

/// Kuma `discord`: `server/notification-providers/discord.js`.
#[derive(Debug, Default, Deserialize)]
pub struct DiscordConfig {
    #[serde(rename = "discordWebhookUrl", default, deserialize_with = "de::secret")]
    pub webhook_url: Option<Secret>,
    #[serde(rename = "discordUsername", default, deserialize_with = "de::text")]
    pub username: Option<String>,
    #[serde(
        rename = "discordPrefixMessage",
        default,
        deserialize_with = "de::text"
    )]
    pub prefix_message: Option<String>,
    #[serde(
        rename = "discordMessageFormat",
        default,
        deserialize_with = "de::choice"
    )]
    pub message_format: Option<DiscordMessageFormat>,
    /// Legacy switch, read only when `message_format` is unset.
    #[serde(
        rename = "discordUseMessageTemplate",
        default,
        deserialize_with = "de::flag"
    )]
    pub use_message_template: bool,
    #[serde(
        rename = "discordMessageTemplate",
        default,
        deserialize_with = "de::text"
    )]
    pub message_template: Option<String>,
    #[serde(
        rename = "discordChannelType",
        default,
        deserialize_with = "de::choice"
    )]
    pub channel_type: Option<DiscordChannelType>,
    #[serde(rename = "threadId", default, deserialize_with = "de::text")]
    pub thread_id: Option<String>,
    #[serde(rename = "postName", default, deserialize_with = "de::text")]
    pub post_name: Option<String>,
    #[serde(rename = "disableUrl", default, deserialize_with = "de::flag")]
    pub disable_url: bool,
    #[serde(
        rename = "discordSuppressNotifications",
        default,
        deserialize_with = "de::flag"
    )]
    pub suppress_notifications: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordMessageFormat {
    Normal,
    Minimalist,
    Custom,
}

impl FromStr for DiscordMessageFormat {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "normal" => Ok(Self::Normal),
            "minimalist" => Ok(Self::Minimalist),
            "custom" => Ok(Self::Custom),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordChannelType {
    Channel,
    CreateNewForumPost,
    PostToThread,
}

impl FromStr for DiscordChannelType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "channel" => Ok(Self::Channel),
            "createNewForumPost" => Ok(Self::CreateNewForumPost),
            "postToThread" => Ok(Self::PostToThread),
            _ => Err(()),
        }
    }
}

/// Kuma `webhook`: `server/notification-providers/webhook.js`.
#[derive(Debug, Default, Deserialize)]
pub struct WebhookConfig {
    #[serde(rename = "webhookURL", default, deserialize_with = "de::secret")]
    pub url: Option<Secret>,
    #[serde(rename = "httpMethod", default, deserialize_with = "de::choice")]
    pub method: Option<WebhookMethod>,
    #[serde(
        rename = "webhookContentType",
        default,
        deserialize_with = "de::choice"
    )]
    pub content_type: Option<WebhookContentType>,
    #[serde(rename = "webhookCustomBody", default, deserialize_with = "de::text")]
    pub custom_body: Option<String>,
    /// A JSON object of extra headers, often with a token.
    #[serde(
        rename = "webhookAdditionalHeaders",
        default,
        deserialize_with = "de::secret"
    )]
    pub additional_headers: Option<Secret>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebhookMethod {
    Post,
    Get,
}

impl FromStr for WebhookMethod {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s.to_ascii_lowercase().as_str() {
            "get" => Ok(Self::Get),
            _ => Ok(Self::Post),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebhookContentType {
    Json,
    FormData,
    Custom,
}

impl FromStr for WebhookContentType {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "json" => Ok(Self::Json),
            "form-data" => Ok(Self::FormData),
            "custom" => Ok(Self::Custom),
            _ => Err(()),
        }
    }
}

/// Kuma `slack`: `server/notification-providers/slack.js`.
#[derive(Debug, Default, Deserialize)]
pub struct SlackConfig {
    #[serde(rename = "slackwebhookURL", default, deserialize_with = "de::secret")]
    pub webhook_url: Option<Secret>,
    #[serde(rename = "slackchannel", default, deserialize_with = "de::text")]
    pub channel: Option<String>,
    #[serde(rename = "slackusername", default, deserialize_with = "de::text")]
    pub username: Option<String>,
    #[serde(rename = "slackiconemo", default, deserialize_with = "de::text")]
    pub icon_emoji: Option<String>,
    /// Appends ` <!channel>` to the message.
    #[serde(rename = "slackchannelnotify", default, deserialize_with = "de::flag")]
    pub channel_notify: bool,
    #[serde(rename = "slackrichmessage", default, deserialize_with = "de::flag")]
    pub rich_message: bool,
    #[serde(rename = "slackUseTemplate", default, deserialize_with = "de::flag")]
    pub use_template: bool,
    #[serde(rename = "slackTemplate", default, deserialize_with = "de::text")]
    pub template: Option<String>,
    /// Kuma defaults this to on when unset.
    #[serde(
        rename = "slackIncludeGroupName",
        default,
        deserialize_with = "de::opt_flag"
    )]
    pub include_group_name: Option<bool>,
}

/// Kuma `teams`: `server/notification-providers/teams.js`.
#[derive(Debug, Default, Deserialize)]
pub struct TeamsConfig {
    #[serde(rename = "webhookUrl", default, deserialize_with = "de::secret")]
    pub webhook_url: Option<Secret>,
    #[serde(rename = "teamsEnableTags", default, deserialize_with = "de::flag")]
    pub enable_tags: bool,
}

/// Kuma `telegram`: `server/notification-providers/telegram.js`.
#[derive(Debug, Default, Deserialize)]
pub struct TelegramConfig {
    #[serde(rename = "telegramBotToken", default, deserialize_with = "de::secret")]
    pub bot_token: Option<Secret>,
    #[serde(rename = "telegramChatID", default, deserialize_with = "de::text")]
    pub chat_id: Option<String>,
    /// Defaults to `https://api.telegram.org`.
    #[serde(rename = "telegramServerUrl", default, deserialize_with = "de::text")]
    pub server_url: Option<String>,
    #[serde(
        rename = "telegramMessageThreadID",
        default,
        deserialize_with = "de::text"
    )]
    pub message_thread_id: Option<String>,
    #[serde(
        rename = "telegramSendSilently",
        default,
        deserialize_with = "de::flag"
    )]
    pub send_silently: bool,
    #[serde(
        rename = "telegramProtectContent",
        default,
        deserialize_with = "de::flag"
    )]
    pub protect_content: bool,
    #[serde(rename = "telegramUseTemplate", default, deserialize_with = "de::flag")]
    pub use_template: bool,
    #[serde(rename = "telegramTemplate", default, deserialize_with = "de::text")]
    pub template: Option<String>,
    #[serde(
        rename = "telegramTemplateParseMode",
        default,
        deserialize_with = "de::choice"
    )]
    pub parse_mode: Option<TelegramParseMode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelegramParseMode {
    Plain,
    Html,
    MarkdownV2,
}

impl FromStr for TelegramParseMode {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "plain" => Ok(Self::Plain),
            "HTML" => Ok(Self::Html),
            "MarkdownV2" => Ok(Self::MarkdownV2),
            _ => Err(()),
        }
    }
}

/// Kuma `ntfy`: `server/notification-providers/ntfy.js`.
#[derive(Debug, Default, Deserialize)]
pub struct NtfyConfig {
    #[serde(rename = "ntfyserverurl", default, deserialize_with = "de::text")]
    pub server_url: Option<String>,
    #[serde(rename = "ntfytopic", default, deserialize_with = "de::text")]
    pub topic: Option<String>,
    #[serde(rename = "ntfyPriority", default, deserialize_with = "de::number")]
    pub priority: Option<u8>,
    #[serde(rename = "ntfyPriorityDown", default, deserialize_with = "de::number")]
    pub priority_down: Option<u8>,
    /// Kuma sends no credentials without a method, even when a username is stored.
    #[serde(
        rename = "ntfyAuthenticationMethod",
        default,
        deserialize_with = "de::choice"
    )]
    pub auth: Option<NtfyAuth>,
    #[serde(rename = "ntfyusername", default, deserialize_with = "de::text")]
    pub username: Option<String>,
    #[serde(rename = "ntfypassword", default, deserialize_with = "de::secret")]
    pub password: Option<Secret>,
    #[serde(rename = "ntfyaccesstoken", default, deserialize_with = "de::secret")]
    pub access_token: Option<Secret>,
    /// Value of the `X-Call` header.
    #[serde(rename = "ntfyCall", default, deserialize_with = "de::text")]
    pub call: Option<String>,
    #[serde(rename = "ntfyIcon", default, deserialize_with = "de::text")]
    pub icon: Option<String>,
    #[serde(rename = "ntfyUseTemplate", default, deserialize_with = "de::flag")]
    pub use_template: bool,
    #[serde(rename = "ntfyCustomTitle", default, deserialize_with = "de::text")]
    pub custom_title: Option<String>,
    #[serde(rename = "ntfyCustomMessage", default, deserialize_with = "de::text")]
    pub custom_message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NtfyAuth {
    None,
    UsernamePassword,
    AccessToken,
}

impl FromStr for NtfyAuth {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "none" => Ok(Self::None),
            "usernamePassword" => Ok(Self::UsernamePassword),
            "accessToken" => Ok(Self::AccessToken),
            _ => Err(()),
        }
    }
}

/// Kuma `pushover`: `server/notification-providers/pushover.js`.
#[derive(Debug, Default, Deserialize)]
pub struct PushoverConfig {
    #[serde(rename = "pushoveruserkey", default, deserialize_with = "de::secret")]
    pub user_key: Option<Secret>,
    #[serde(rename = "pushoverapptoken", default, deserialize_with = "de::secret")]
    pub app_token: Option<Secret>,
    #[serde(rename = "pushoversounds", default, deserialize_with = "de::text")]
    pub sound: Option<String>,
    #[serde(rename = "pushoversounds_up", default, deserialize_with = "de::text")]
    pub sound_up: Option<String>,
    #[serde(rename = "pushoverpriority", default, deserialize_with = "de::number")]
    pub priority: Option<i8>,
    #[serde(rename = "pushovertitle", default, deserialize_with = "de::text")]
    pub title: Option<String>,
    #[serde(rename = "pushoverdevice", default, deserialize_with = "de::text")]
    pub device: Option<String>,
    #[serde(rename = "pushoverttl", default, deserialize_with = "de::number")]
    pub ttl: Option<u64>,
}
