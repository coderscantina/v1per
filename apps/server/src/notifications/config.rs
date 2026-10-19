//! Channel settings between the protocol's typed form and Kuma's `notification.config` JSON.
//! The JSON is what is sealed, what the importer writes and what `v1per_notify` reads, so an
//! imported channel and one made in v1per send alike.

use serde_json::{Map, Value};
use v1per_notify as notify;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::ChannelId;
use v1per_protocol::operations::channels::{
    ChannelSettings, DiscordChannel, DiscordChannelType, DiscordMessageFormat, NtfyAuth,
    NtfyChannel, PushoverChannel, SlackChannel, SmtpChannel, TeamsChannel, TelegramChannel,
    TelegramParseMode, UnsupportedChannel, WebhookChannel, WebhookContentType, WebhookMethod,
};
use v1per_protocol::operations::monitors::SecretField;

use crate::error::{ApiError, FieldErrors};
use crate::identity::secrets::SecretKey;

pub(crate) type Kuma = Map<String, Value>;

/// The sealing context of a channel's config.
pub(crate) fn channel_context(channel: ChannelId) -> String {
    format!("channel:{channel}:config")
}

pub(crate) fn seal(key: &SecretKey, context: &str, config: &Kuma) -> Vec<u8> {
    key.seal(
        context,
        Value::Object(config.clone()).to_string().as_bytes(),
    )
}

/// The config sealed under `context`. A ciphertext that does not open is an internal error:
/// the key changed or the row was copied.
pub(crate) fn open(key: &SecretKey, context: &str, sealed: &[u8]) -> anyhow::Result<Kuma> {
    let plain = key
        .open(context, sealed)
        .ok_or_else(|| anyhow::anyhow!("sealed notification config {context} does not open"))?;
    Ok(serde_json::from_slice(&plain)?)
}

/// Kuma's `type` of the settings.
pub(crate) fn provider(settings: &ChannelSettings) -> &str {
    match settings {
        ChannelSettings::Smtp(_) => "smtp",
        ChannelSettings::Discord(_) => "discord",
        ChannelSettings::Webhook(_) => "webhook",
        ChannelSettings::Slack(_) => "slack",
        ChannelSettings::Teams(_) => "teams",
        ChannelSettings::Telegram(_) => "telegram",
        ChannelSettings::Ntfy(_) => "ntfy",
        ChannelSettings::Pushover(_) => "pushover",
        ChannelSettings::Unsupported(unsupported) => &unsupported.kuma_type,
    }
}

/// Builds Kuma's JSON field by field and collects what is invalid.
struct Builder<'a> {
    map: Kuma,
    stored: Option<&'a Kuma>,
    errors: FieldErrors,
}

impl Builder<'_> {
    fn text(&mut self, key: &str, value: &Option<String>) {
        if let Some(value) = value.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            self.map.insert(key.into(), value.into());
        }
    }

    fn flag(&mut self, key: &str, value: bool) {
        self.map.insert(key.into(), value.into());
    }

    fn value(&mut self, key: &str, value: Option<impl Into<Value>>) {
        if let Some(value) = value {
            self.map.insert(key.into(), value.into());
        }
    }

    /// A secret: the new value, nothing when cleared, else the stored value.
    fn secret(&mut self, key: &str, field: &SecretField) {
        let value = match (&field.value, field.clear) {
            (Some(value), _) => Some(Value::String(value.clone())),
            (None, true) => None,
            (None, false) => self.stored.and_then(|stored| stored.get(key)).cloned(),
        };
        if let Some(value) = value.filter(|v| v.as_str().is_none_or(|v| !v.is_empty())) {
            self.map.insert(key.into(), value);
        }
    }

    /// A Liquid template, checked so a broken one is refused here rather than at send time.
    fn template(&mut self, key: &str, field: &str, value: &Option<String>) {
        if let Some(source) = value.as_deref().filter(|v| !v.trim().is_empty()) {
            if notify::check_template(source).is_err() {
                self.errors.add(field, FieldErrorCode::InvalidFormat, None);
            }
            self.map.insert(key.into(), source.into());
        }
    }

    /// `field` must have produced `key`.
    fn require(&mut self, key: &str, field: &str) {
        if !self.map.contains_key(key) {
            self.errors.add(field, FieldErrorCode::Required, None);
        }
    }

    /// `key`, when set, must be an http or https URL.
    fn url(&mut self, key: &str, field: &str) {
        let valid = |text: &str| {
            url::Url::parse(text).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
        };
        if let Some(text) = self.map.get(key).and_then(Value::as_str)
            && !valid(text)
        {
            self.errors.add(field, FieldErrorCode::InvalidFormat, None);
        }
    }
}

/// Kuma's config JSON for `settings` named `name`. A secret field without a value keeps the
/// value `stored` holds under the same key. `unsupported` settings are refused.
pub(crate) fn to_kuma(
    name: &str,
    settings: &ChannelSettings,
    stored: Option<&Kuma>,
) -> Result<Kuma, ApiError> {
    let mut b = Builder {
        map: Map::new(),
        stored,
        errors: FieldErrors::default(),
    };
    b.map.insert("name".into(), name.into());
    b.map.insert("type".into(), provider(settings).into());
    match settings {
        ChannelSettings::Smtp(c) => {
            b.text("smtpHost", &c.host);
            b.value("smtpPort", c.port);
            b.flag("smtpSecure", c.secure);
            b.flag("smtpIgnoreTLSError", c.ignore_tls_error);
            b.flag("smtpIgnoreSTARTTLS", c.ignore_starttls);
            b.text("smtpUsername", &c.username);
            b.secret("smtpPassword", &c.password);
            b.text("smtpFrom", &c.from);
            b.text("smtpTo", &c.to);
            b.text("smtpCC", &c.cc);
            b.text("smtpBCC", &c.bcc);
            b.template(
                "customSubject",
                "settings.custom_subject",
                &c.custom_subject,
            );
            b.template("customBody", "settings.custom_body", &c.custom_body);
            b.flag("htmlBody", c.html_body);
            b.secret("smtpAdditionalHeaders", &c.additional_headers);
            b.text("smtpDkimDomain", &c.dkim_domain);
            b.text("smtpDkimKeySelector", &c.dkim_key_selector);
            b.secret("smtpDkimPrivateKey", &c.dkim_private_key);
            b.text("smtpDkimHashAlgo", &c.dkim_hash_algo);
            b.text("smtpDkimheaderFieldNames", &c.dkim_header_field_names);
            b.text("smtpDkimskipFields", &c.dkim_skip_fields);
            if !["smtpTo", "smtpCC", "smtpBCC"]
                .iter()
                .any(|key| b.map.contains_key(*key))
            {
                b.errors.add("settings.to", FieldErrorCode::Required, None);
            }
            if b.map.contains_key("smtpHost") && !b.map.contains_key("smtpFrom") {
                b.errors
                    .add("settings.from", FieldErrorCode::Required, None);
            }
        }
        ChannelSettings::Discord(c) => {
            b.secret("discordWebhookUrl", &c.webhook_url);
            b.require("discordWebhookUrl", "settings.webhook_url");
            b.url("discordWebhookUrl", "settings.webhook_url");
            b.text("discordUsername", &c.username);
            b.text("discordPrefixMessage", &c.prefix_message);
            b.value(
                "discordMessageFormat",
                c.message_format.map(|format| match format {
                    DiscordMessageFormat::Normal => "normal",
                    DiscordMessageFormat::Minimalist => "minimalist",
                    DiscordMessageFormat::Custom => "custom",
                }),
            );
            b.flag("discordUseMessageTemplate", c.use_message_template);
            b.template(
                "discordMessageTemplate",
                "settings.message_template",
                &c.message_template,
            );
            b.value(
                "discordChannelType",
                c.channel_type.map(|kind| match kind {
                    DiscordChannelType::Channel => "channel",
                    DiscordChannelType::CreateNewForumPost => "createNewForumPost",
                    DiscordChannelType::PostToThread => "postToThread",
                }),
            );
            b.text("threadId", &c.thread_id);
            b.text("postName", &c.post_name);
            b.flag("disableUrl", c.disable_url);
            b.flag("discordSuppressNotifications", c.suppress_notifications);
        }
        ChannelSettings::Webhook(c) => {
            b.secret("webhookURL", &c.url);
            b.require("webhookURL", "settings.url");
            b.url("webhookURL", "settings.url");
            b.value(
                "httpMethod",
                c.method.map(|method| match method {
                    WebhookMethod::Post => "post",
                    WebhookMethod::Get => "get",
                }),
            );
            b.value(
                "webhookContentType",
                c.content_type.map(|kind| match kind {
                    WebhookContentType::Json => "json",
                    WebhookContentType::FormData => "form-data",
                    WebhookContentType::Custom => "custom",
                }),
            );
            b.template("webhookCustomBody", "settings.custom_body", &c.custom_body);
            b.secret("webhookAdditionalHeaders", &c.additional_headers);
        }
        ChannelSettings::Slack(c) => {
            b.secret("slackwebhookURL", &c.webhook_url);
            b.require("slackwebhookURL", "settings.webhook_url");
            b.url("slackwebhookURL", "settings.webhook_url");
            b.text("slackchannel", &c.channel);
            b.text("slackusername", &c.username);
            b.text("slackiconemo", &c.icon_emoji);
            b.flag("slackchannelnotify", c.channel_notify);
            b.flag("slackrichmessage", c.rich_message);
            b.flag("slackUseTemplate", c.use_template);
            b.template("slackTemplate", "settings.template", &c.template);
            b.value("slackIncludeGroupName", c.include_group_name);
        }
        ChannelSettings::Teams(c) => {
            b.secret("webhookUrl", &c.webhook_url);
            b.require("webhookUrl", "settings.webhook_url");
            b.url("webhookUrl", "settings.webhook_url");
            b.flag("teamsEnableTags", c.enable_tags);
        }
        ChannelSettings::Telegram(c) => {
            b.secret("telegramBotToken", &c.bot_token);
            b.require("telegramBotToken", "settings.bot_token");
            b.text("telegramChatID", &c.chat_id);
            b.require("telegramChatID", "settings.chat_id");
            b.text("telegramServerUrl", &c.server_url);
            b.url("telegramServerUrl", "settings.server_url");
            b.text("telegramMessageThreadID", &c.message_thread_id);
            b.flag("telegramSendSilently", c.send_silently);
            b.flag("telegramProtectContent", c.protect_content);
            b.flag("telegramUseTemplate", c.use_template);
            b.template("telegramTemplate", "settings.template", &c.template);
            b.value(
                "telegramTemplateParseMode",
                c.parse_mode.map(|mode| match mode {
                    TelegramParseMode::Plain => "plain",
                    TelegramParseMode::Html => "HTML",
                    TelegramParseMode::MarkdownV2 => "MarkdownV2",
                }),
            );
        }
        ChannelSettings::Ntfy(c) => {
            b.text("ntfyserverurl", &c.server_url);
            b.url("ntfyserverurl", "settings.server_url");
            b.text("ntfytopic", &c.topic);
            b.require("ntfytopic", "settings.topic");
            for (key, field, priority) in [
                ("ntfyPriority", "settings.priority", c.priority),
                (
                    "ntfyPriorityDown",
                    "settings.priority_down",
                    c.priority_down,
                ),
            ] {
                if priority.is_some_and(|p| !(1..=5).contains(&p)) {
                    b.errors.add(field, FieldErrorCode::InvalidValue, None);
                }
                b.value(key, priority);
            }
            b.value(
                "ntfyAuthenticationMethod",
                c.auth.map(|auth| match auth {
                    NtfyAuth::None => "none",
                    NtfyAuth::UsernamePassword => "usernamePassword",
                    NtfyAuth::AccessToken => "accessToken",
                }),
            );
            b.text("ntfyusername", &c.username);
            b.secret("ntfypassword", &c.password);
            b.secret("ntfyaccesstoken", &c.access_token);
            b.text("ntfyCall", &c.call);
            b.text("ntfyIcon", &c.icon);
            b.flag("ntfyUseTemplate", c.use_template);
            b.template("ntfyCustomTitle", "settings.custom_title", &c.custom_title);
            b.template(
                "ntfyCustomMessage",
                "settings.custom_message",
                &c.custom_message,
            );
        }
        ChannelSettings::Pushover(c) => {
            b.secret("pushoveruserkey", &c.user_key);
            b.require("pushoveruserkey", "settings.user_key");
            b.secret("pushoverapptoken", &c.app_token);
            b.require("pushoverapptoken", "settings.app_token");
            b.text("pushoversounds", &c.sound);
            b.text("pushoversounds_up", &c.sound_up);
            if c.priority.is_some_and(|p| !(-2..=2).contains(&p)) {
                b.errors
                    .add("settings.priority", FieldErrorCode::InvalidValue, None);
            }
            b.value("pushoverpriority", c.priority);
            b.text("pushovertitle", &c.title);
            b.text("pushoverdevice", &c.device);
            b.value("pushoverttl", c.ttl);
        }
        ChannelSettings::Unsupported(_) => {
            b.errors.add("settings", FieldErrorCode::InvalidValue, None);
        }
    }
    b.errors.finish()?;
    Ok(b.map)
}

fn set(secret: &Option<notify::Secret>) -> SecretField {
    SecretField {
        set: secret.is_some(),
        ..SecretField::default()
    }
}

/// The typed settings of Kuma's config, secrets shown only as set. `None` when the config
/// does not parse, which only a broken import leaves behind.
pub(crate) fn from_kuma(config: &Kuma) -> Option<ChannelSettings> {
    let parsed: notify::ChannelConfig =
        serde_json::from_value(Value::Object(config.clone())).ok()?;
    Some(match parsed {
        notify::ChannelConfig::Smtp(c) => ChannelSettings::Smtp(SmtpChannel {
            host: c.host,
            port: c.port,
            secure: c.secure,
            ignore_tls_error: c.ignore_tls_error,
            ignore_starttls: c.ignore_starttls,
            username: c.username,
            password: set(&c.password),
            from: c.from,
            to: c.to,
            cc: c.cc,
            bcc: c.bcc,
            custom_subject: c.custom_subject,
            custom_body: c.custom_body,
            html_body: c.html_body,
            additional_headers: set(&c.additional_headers),
            dkim_domain: c.dkim_domain,
            dkim_key_selector: c.dkim_key_selector,
            dkim_private_key: set(&c.dkim_private_key),
            dkim_hash_algo: c.dkim_hash_algo,
            dkim_header_field_names: c.dkim_header_field_names,
            dkim_skip_fields: c.dkim_skip_fields,
        }),
        notify::ChannelConfig::Discord(c) => ChannelSettings::Discord(DiscordChannel {
            webhook_url: set(&c.webhook_url),
            username: c.username,
            prefix_message: c.prefix_message,
            message_format: c.message_format.map(|format| match format {
                notify::DiscordMessageFormat::Normal => DiscordMessageFormat::Normal,
                notify::DiscordMessageFormat::Minimalist => DiscordMessageFormat::Minimalist,
                notify::DiscordMessageFormat::Custom => DiscordMessageFormat::Custom,
            }),
            use_message_template: c.use_message_template,
            message_template: c.message_template,
            channel_type: c.channel_type.map(|kind| match kind {
                notify::DiscordChannelType::Channel => DiscordChannelType::Channel,
                notify::DiscordChannelType::CreateNewForumPost => {
                    DiscordChannelType::CreateNewForumPost
                }
                notify::DiscordChannelType::PostToThread => DiscordChannelType::PostToThread,
            }),
            thread_id: c.thread_id,
            post_name: c.post_name,
            disable_url: c.disable_url,
            suppress_notifications: c.suppress_notifications,
        }),
        notify::ChannelConfig::Webhook(c) => ChannelSettings::Webhook(WebhookChannel {
            url: set(&c.url),
            method: c.method.map(|method| match method {
                notify::WebhookMethod::Post => WebhookMethod::Post,
                notify::WebhookMethod::Get => WebhookMethod::Get,
            }),
            content_type: c.content_type.map(|kind| match kind {
                notify::WebhookContentType::Json => WebhookContentType::Json,
                notify::WebhookContentType::FormData => WebhookContentType::FormData,
                notify::WebhookContentType::Custom => WebhookContentType::Custom,
            }),
            custom_body: c.custom_body,
            additional_headers: set(&c.additional_headers),
        }),
        notify::ChannelConfig::Slack(c) => ChannelSettings::Slack(SlackChannel {
            webhook_url: set(&c.webhook_url),
            channel: c.channel,
            username: c.username,
            icon_emoji: c.icon_emoji,
            channel_notify: c.channel_notify,
            rich_message: c.rich_message,
            use_template: c.use_template,
            template: c.template,
            include_group_name: c.include_group_name,
        }),
        notify::ChannelConfig::Teams(c) => ChannelSettings::Teams(TeamsChannel {
            webhook_url: set(&c.webhook_url),
            enable_tags: c.enable_tags,
        }),
        notify::ChannelConfig::Telegram(c) => ChannelSettings::Telegram(TelegramChannel {
            bot_token: set(&c.bot_token),
            chat_id: c.chat_id,
            server_url: c.server_url,
            message_thread_id: c.message_thread_id,
            send_silently: c.send_silently,
            protect_content: c.protect_content,
            use_template: c.use_template,
            template: c.template,
            parse_mode: c.parse_mode.map(|mode| match mode {
                notify::TelegramParseMode::Plain => TelegramParseMode::Plain,
                notify::TelegramParseMode::Html => TelegramParseMode::Html,
                notify::TelegramParseMode::MarkdownV2 => TelegramParseMode::MarkdownV2,
            }),
        }),
        notify::ChannelConfig::Ntfy(c) => ChannelSettings::Ntfy(NtfyChannel {
            server_url: c.server_url,
            topic: c.topic,
            priority: c.priority,
            priority_down: c.priority_down,
            auth: c.auth.map(|auth| match auth {
                notify::NtfyAuth::None => NtfyAuth::None,
                notify::NtfyAuth::UsernamePassword => NtfyAuth::UsernamePassword,
                notify::NtfyAuth::AccessToken => NtfyAuth::AccessToken,
            }),
            username: c.username,
            password: set(&c.password),
            access_token: set(&c.access_token),
            call: c.call,
            icon: c.icon,
            use_template: c.use_template,
            custom_title: c.custom_title,
            custom_message: c.custom_message,
        }),
        notify::ChannelConfig::Pushover(c) => ChannelSettings::Pushover(PushoverChannel {
            user_key: set(&c.user_key),
            app_token: set(&c.app_token),
            sound: c.sound,
            sound_up: c.sound_up,
            priority: c.priority,
            title: c.title,
            device: c.device,
            ttl: c.ttl,
        }),
        notify::ChannelConfig::Unsupported(c) => ChannelSettings::Unsupported(UnsupportedChannel {
            kuma_type: c.kind().to_owned(),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form round-trips through Kuma's JSON, secrets kept from the stored config unless
    /// replaced or cleared, and never shown.
    #[test]
    fn settings_round_trip_with_stored_secrets() {
        let form = ChannelSettings::Discord(DiscordChannel {
            webhook_url: SecretField {
                value: Some("https://discord.test/api/webhooks/1/abc".into()),
                ..SecretField::default()
            },
            username: Some("v1per".into()),
            message_format: Some(DiscordMessageFormat::Custom),
            message_template: Some("{{ msg }}".into()),
            channel_type: Some(DiscordChannelType::PostToThread),
            ..DiscordChannel::default()
        });
        let kuma = to_kuma("Ops", &form, None).unwrap();
        assert_eq!(kuma["type"], "discord");
        assert_eq!(kuma["discordChannelType"], "postToThread");
        let ChannelSettings::Discord(shown) = from_kuma(&kuma).unwrap() else {
            panic!("discord");
        };
        assert!(shown.webhook_url.set && shown.webhook_url.value.is_none());
        assert_eq!(shown.username.as_deref(), Some("v1per"));

        let kept = to_kuma("Ops", &ChannelSettings::Discord(shown.clone()), Some(&kuma)).unwrap();
        assert_eq!(kept["discordWebhookUrl"], kuma["discordWebhookUrl"]);
        let cleared = ChannelSettings::Discord(DiscordChannel {
            webhook_url: SecretField {
                clear: true,
                ..SecretField::default()
            },
            ..shown
        });
        let refused = to_kuma("Ops", &cleared, Some(&kuma)).unwrap_err();
        assert_eq!(
            serde_json::to_value(refused.body()).unwrap()["errors"]["settings.webhook_url"][0]["code"],
            "required"
        );
    }

    #[test]
    fn broken_templates_and_missing_recipients_are_refused() {
        let form = ChannelSettings::Smtp(SmtpChannel {
            custom_body: Some("{% if %}".into()),
            ..SmtpChannel::default()
        });
        let error = to_kuma("Mail", &form, None).unwrap_err();
        let errors = &serde_json::to_value(error.body()).unwrap()["errors"];
        assert_eq!(errors["settings.custom_body"][0]["code"], "invalid_format");
        assert_eq!(errors["settings.to"][0]["code"], "required");
    }
}
