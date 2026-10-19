//! Kuma `notification` rows to channels (`docs/contracts/kuma-notifications.md`).

use serde::Deserialize;
use serde_json::{Map, Value as Json};
use v1per_checks::Secret;
use v1per_notify::{ChannelConfig, DiscordMessageFormat, WebhookContentType, check_template};

use crate::plan::{ChannelPlan, ChannelState, NotificationId};
use crate::record::Record;

/// The 98 provider keys Kuma 2.5.0 registers (`server/notification.js:117-216`).
const KUMA_PROVIDERS: [&str; 98] = [
    "alerta",
    "AlertNow",
    "AliyunSMS",
    "apprise",
    "bale",
    "Bark",
    "Bitrix24",
    "Brevo",
    "CallMeBot",
    "Cellsynt",
    "clicksendsms",
    "DingDing",
    "discord",
    "egosms",
    "Elks",
    "evolution",
    "Feishu",
    "FlashDuty",
    "Flowtriq",
    "fluxer",
    "FreeMobile",
    "GoAlert",
    "GoogleChat",
    "GoogleSheets",
    "gorush",
    "gotify",
    "GrafanaOncall",
    "gtxmessaging",
    "HaloPSA",
    "HeiiOnCall",
    "HomeAssistant",
    "JiraServiceManagement",
    "Keep",
    "Kook",
    "line",
    "lunasea",
    "matrix",
    "mattermost",
    "max",
    "nextcloudtalk",
    "nostr",
    "notifery",
    "ntfy",
    "octopush",
    "OneBot",
    "OneChat",
    "Onesender",
    "Ooredoo",
    "Opsgenie",
    "PagerDuty",
    "PagerTree",
    "plivo",
    "promosms",
    "pumble",
    "pushbullet",
    "PushByTechulus",
    "PushDeer",
    "pushover",
    "PushPlus",
    "pushy",
    "Resend",
    "rocket.chat",
    "SendGrid",
    "ServerChan",
    "serwersms",
    "SevenIO",
    "signal",
    "SIGNL4",
    "slack",
    "smsc",
    "SMSEagle",
    "smsir",
    "SMSManager",
    "SMSPartner",
    "SMSPlanet",
    "smtp",
    "Splunk",
    "SpugPush",
    "squadcast",
    "stackfield",
    "teams",
    "telegram",
    "telnyx",
    "Teltonika",
    "threema",
    "twilio",
    "VK",
    "VKTeams",
    "waha",
    "webhook",
    "Webpush",
    "WeCom",
    "whapi",
    "Whatsapp360messenger",
    "WPush",
    "WxPusher",
    "YZJ",
    "ZohoCliq",
];

pub(crate) fn channel(record: &Record) -> ChannelPlan {
    let id = NotificationId(record.id());
    let name = record.text("name").unwrap_or_default();
    let is_default = record.flag("is_default");
    let parsed = record
        .text("config")
        .and_then(|text| serde_json::from_str::<Map<String, Json>>(&text).ok());
    let Some(mut config) = parsed else {
        return ChannelPlan {
            id,
            name,
            provider: String::new(),
            config: None,
            sealed_config: Secret::new(record.text("config").unwrap_or_default()),
            is_default,
            state: ChannelState::Inactive {
                reason: "config is not a JSON object".to_owned(),
            },
        };
    };
    // Form state only: `is_default` is a column and `applyExisting` acted once on save.
    config.remove("isDefault");
    config.remove("applyExisting");
    let provider = match config.get("type") {
        Some(Json::String(kind)) => kind.clone(),
        _ => String::new(),
    };
    let sealed_config = Secret::new(Json::Object(config.clone()).to_string());
    let (config, state) = match ChannelConfig::deserialize(Json::Object(config)) {
        Ok(config) => {
            let state = state(&config, &provider);
            (Some(config), state)
        }
        Err(error) => (
            None,
            ChannelState::Inactive {
                reason: format!("config does not parse: {error}"),
            },
        ),
    };
    ChannelPlan {
        id,
        name,
        provider,
        config,
        sealed_config,
        is_default,
        state,
    }
}

fn state(config: &ChannelConfig, provider: &str) -> ChannelState {
    let inactive = |reason: String| ChannelState::Inactive { reason };
    if matches!(config, ChannelConfig::Unsupported(_)) {
        return inactive(if KUMA_PROVIDERS.contains(&provider) {
            "provider not supported yet".to_owned()
        } else {
            "unknown provider type".to_owned()
        });
    }
    match used_templates(config)
        .into_iter()
        .flatten()
        .find_map(|template| check_template(template).err())
    {
        Some(error) => inactive(format!("template does not parse: {error}")),
        None => ChannelState::Active,
    }
}

/// The Liquid templates the provider renders with this config, as Kuma decides.
fn used_templates(config: &ChannelConfig) -> Vec<Option<&str>> {
    match config {
        ChannelConfig::Smtp(smtp) => {
            vec![smtp.custom_subject.as_deref(), smtp.custom_body.as_deref()]
        }
        ChannelConfig::Discord(discord) => {
            let custom = discord.message_format == Some(DiscordMessageFormat::Custom)
                || (discord.message_format.is_none() && discord.use_message_template);
            vec![when(custom, &discord.message_template)]
        }
        ChannelConfig::Webhook(webhook) => vec![when(
            webhook.content_type == Some(WebhookContentType::Custom),
            &webhook.custom_body,
        )],
        ChannelConfig::Slack(slack) => vec![when(slack.use_template, &slack.template)],
        ChannelConfig::Telegram(telegram) => vec![when(telegram.use_template, &telegram.template)],
        ChannelConfig::Ntfy(ntfy) => vec![
            when(ntfy.use_template, &ntfy.custom_title),
            when(ntfy.use_template, &ntfy.custom_message),
        ],
        ChannelConfig::Teams(_) | ChannelConfig::Pushover(_) | ChannelConfig::Unsupported(_) => {
            Vec::new()
        }
    }
}

fn when(on: bool, template: &Option<String>) -> Option<&str> {
    template.as_deref().filter(|_| on)
}
