//! Kuma `telegram.js`.

use serde_json::{Map, Value, json};

use crate::config::{TelegramConfig, TelegramParseMode};
use crate::error::SendError;
use crate::event::Notice;
use crate::js::Js;
use crate::{Delivered, SendContext, http, template};

const DEFAULT_SERVER: &str = "https://api.telegram.org";

pub(crate) async fn send(
    config: &TelegramConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let token = config
        .bot_token
        .as_ref()
        .ok_or_else(|| SendError::Config("Telegram bot token is missing".into()))?;
    let server = config.server_url.as_deref().unwrap_or(DEFAULT_SERVER);
    let endpoint = format!("{server}/bot{}/sendMessage", token.expose());
    let url = http::url(Some(&endpoint), "Telegram server URL")?;
    let payload = payload(config, notice)?;
    http::deliver(ctx.http.post(url).json(&payload)).await
}

fn payload(config: &TelegramConfig, notice: &Notice<'_>) -> Result<Map<String, Value>, SendError> {
    let mut payload = Map::new();
    if let Some(chat_id) = &config.chat_id {
        payload.insert("chat_id".into(), chat_id.as_str().into());
    }
    payload.insert("text".into(), notice.msg.as_str().into());
    payload.insert("disable_notification".into(), config.send_silently.into());
    payload.insert("protect_content".into(), config.protect_content.into());
    payload.insert(
        "link_preview_options".into(),
        json!({ "is_disabled": true }),
    );
    if let Some(thread_id) = &config.message_thread_id {
        payload.insert("message_thread_id".into(), thread_id.as_str().into());
    }

    if config.use_template {
        let template = config.template.as_deref().unwrap_or_default();
        let text = if config.parse_mode == Some(TelegramParseMode::MarkdownV2) {
            // Kuma escapes every value the template can reach (`telegram.js:71-88`).
            let monitor = if notice.monitor_json.is_null() {
                Js::object([
                    ("name", escape("Monitor Name not available").into()),
                    ("hostname", escape("testing.hostname").into()),
                    ("url", escape("testing.hostname").into()),
                ])
            } else {
                notice.monitor_json.map_strings(&escape)
            };
            let heartbeat = notice.heartbeat_json.map_strings(&escape);
            template::render(template, &escape(&notice.msg), &monitor, &heartbeat)?
        } else {
            template::render(
                template,
                &notice.msg,
                &notice.monitor_json,
                &notice.heartbeat_json,
            )?
        };
        payload.insert("text".into(), text.into());
        let mode = match config.parse_mode {
            Some(TelegramParseMode::Html) => Some("HTML"),
            Some(TelegramParseMode::MarkdownV2) => Some("MarkdownV2"),
            Some(TelegramParseMode::Plain) | None => None,
        };
        if let Some(mode) = mode {
            payload.insert("parse_mode".into(), mode.into());
        }
    }
    Ok(payload)
}

/// Telegram MarkdownV2 escaping (`telegram.js:12-20`).
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if "_*[]()~>#+-=|{}.!\\".contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}
