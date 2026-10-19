//! Notification providers with Uptime Kuma 2.5.0 output parity.
//!
//! Given a channel config (secrets already unsealed) and an event, [`send`] delivers one
//! message. Outbox, retries, resend intervals and maintenance suppression belong to the caller;
//! [`SendError::is_retryable`] tells it whether trying again can help.
//! The parity contract is `docs/contracts/kuma-notifications.md`.

mod config;
mod de;
mod error;
mod event;
mod http;
mod js;
mod providers;
mod secret;
mod template;

use std::time::Duration;

pub use config::{
    ChannelConfig, DiscordChannelType, DiscordConfig, DiscordMessageFormat, NtfyAuth, NtfyConfig,
    PushoverConfig, SlackConfig, SmtpConfig, TeamsConfig, TelegramConfig, TelegramParseMode,
    UnsupportedConfig, WebhookConfig, WebhookContentType, WebhookMethod,
};
pub use error::SendError;
pub use event::{
    Beat, CertType, CertificateExpiry, DomainExpiry, Event, Heartbeat, Monitor, MonitorTag,
    ProbeOffline,
};
pub use secret::Secret;
pub use template::{TemplateError, check_template};

/// Shared resources and limits for sending. Build once and reuse; it holds the HTTP client.
#[derive(Clone, Debug)]
pub struct SendContext {
    http: reqwest::Client,
    timeout: Duration,
    pushover_endpoint: String,
}

impl SendContext {
    /// A context with a 30 second hard timeout per send.
    pub fn new() -> Self {
        Self {
            http: http::client(),
            timeout: Duration::from_secs(30),
            pushover_endpoint: providers::pushover::ENDPOINT.to_owned(),
        }
    }

    /// Caps the whole send, including connect, TLS, the Discord avatar check and the request.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Replaces Kuma's fixed Pushover API URL, for a relay or a test server.
    pub fn with_pushover_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.pushover_endpoint = endpoint.into();
        self
    }
}

impl Default for SendContext {
    fn default() -> Self {
        Self::new()
    }
}

/// What the provider answered, for the delivery log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivered {
    pub detail: String,
}

/// Delivers `event` through `channel`, bounded by the context's timeout.
pub async fn send(
    channel: &ChannelConfig,
    event: &Event,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let notice = event.notice();
    let delivery = async {
        match channel {
            ChannelConfig::Smtp(config) => providers::smtp::send(config, &notice, ctx).await,
            ChannelConfig::Discord(config) => providers::discord::send(config, &notice, ctx).await,
            ChannelConfig::Webhook(config) => providers::webhook::send(config, &notice, ctx).await,
            ChannelConfig::Slack(config) => providers::slack::send(config, &notice, ctx).await,
            ChannelConfig::Teams(config) => providers::teams::send(config, &notice, ctx).await,
            ChannelConfig::Telegram(config) => {
                providers::telegram::send(config, &notice, ctx).await
            }
            ChannelConfig::Ntfy(config) => providers::ntfy::send(config, &notice, ctx).await,
            ChannelConfig::Pushover(config) => {
                providers::pushover::send(config, &notice, ctx).await
            }
            ChannelConfig::Unsupported(config) => {
                Err(SendError::Unsupported(config.kind().to_owned()))
            }
        }
    };
    tokio::time::timeout(ctx.timeout, delivery)
        .await
        .map_err(|_| SendError::Timeout(ctx.timeout))?
}
