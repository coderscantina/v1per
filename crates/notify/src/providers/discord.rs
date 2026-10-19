//! Kuma `discord.js`. Payload keys, embed titles, colours and field order match Kuma 2.5.0.

use serde_json::{Map, Value, json};

use crate::config::{DiscordChannelType, DiscordConfig, DiscordMessageFormat};
use crate::error::SendError;
use crate::event::{BeatNotice, Notice, extract_address};
use crate::providers::trimmed;
use crate::{Delivered, SendContext, http, template};

const AVATAR_URL: &str = "https://github.com/louislam/uptime-kuma/raw/master/public/icon.png";
/// Discord's SUPPRESS_NOTIFICATIONS message flag.
const SUPPRESS_NOTIFICATIONS: u32 = 1 << 12;

pub(crate) async fn send(
    config: &DiscordConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let mut url = http::secret_url(config.webhook_url.as_ref(), "Discord webhook URL")?;
    if config.channel_type == Some(DiscordChannelType::PostToThread) {
        let thread_id = config
            .thread_id
            .as_deref()
            .ok_or_else(|| SendError::Config("Discord thread ID is missing".into()))?;
        url.query_pairs_mut().append_pair("thread_id", thread_id);
    }
    // Render before the avatar check so a broken template costs no request.
    let mut payload = payload(config, notice)?;

    // Kuma sets its own avatar unless the webhook has one; when unsure it assumes one.
    let has_avatar = match ctx.http.get(url.clone()).send().await {
        Ok(response) if response.status().is_success() => response
            .json::<Value>()
            .await
            .is_ok_and(|info| info.get("avatar").is_some_and(truthy)),
        _ => true,
    };
    if !has_avatar {
        payload.insert("avatar_url".into(), AVATAR_URL.into());
    }

    http::deliver(ctx.http.post(url).json(&payload)).await
}

fn payload(config: &DiscordConfig, notice: &Notice<'_>) -> Result<Map<String, Value>, SendError> {
    let format = config
        .message_format
        .unwrap_or(if config.use_message_template {
            DiscordMessageFormat::Custom
        } else {
            DiscordMessageFormat::Normal
        });
    let template = trimmed(config.message_template.as_deref());
    let render = |template| {
        template::render(
            template,
            &notice.msg,
            &notice.monitor_json,
            &notice.heartbeat_json,
        )
    };

    let mut payload = Map::new();
    payload.insert(
        "username".into(),
        config.username.as_deref().unwrap_or("Uptime Kuma").into(),
    );

    match (&notice.beat, format, template) {
        // Test, certificate and domain messages: plain content, no prefix.
        (None, DiscordMessageFormat::Minimalist, _) => {
            payload.insert("content".into(), format!("Test: {}", notice.msg).into());
        }
        (None, DiscordMessageFormat::Custom, Some(template)) => {
            payload.insert("content".into(), render(template)?.into());
        }
        (None, _, _) => {
            payload.insert("content".into(), notice.msg.as_str().into());
        }
        (Some(beat), DiscordMessageFormat::Minimalist, _) => {
            let name = &beat.monitor.name;
            let content = if beat.up {
                format!("🟢 {name} is up.")
            } else {
                format!("🔴 {name} is down.")
            };
            payload.insert("content".into(), content.into());
        }
        (Some(_), DiscordMessageFormat::Custom, Some(template)) => {
            payload.insert("content".into(), render(template)?.into());
        }
        (Some(beat), _, _) => {
            payload.insert("embeds".into(), json!([embed(config, notice, beat)]));
            if let Some(prefix) = &config.prefix_message {
                payload.insert("content".into(), prefix.as_str().into());
            }
        }
    }

    if config.channel_type == Some(DiscordChannelType::CreateNewForumPost)
        && let Some(post_name) = &config.post_name
    {
        payload.insert("thread_name".into(), post_name.as_str().into());
    }
    if config.suppress_notifications {
        payload.insert("flags".into(), SUPPRESS_NOTIFICATIONS.into());
    }
    Ok(payload)
}

/// The normal up or down embed (`discord.js:122-257`).
fn embed(config: &DiscordConfig, notice: &Notice<'_>, beat: &BeatNotice<'_>) -> Value {
    let monitor = beat.monitor;
    let mut fields = vec![field("Service Name", &monitor.name)];

    let address = extract_address(&notice.monitor_json);
    if !config.disable_url && !address.is_empty() {
        let name = if monitor.kind == "push" {
            "Service Type"
        } else {
            "Service URL"
        };
        fields.push(field(name, &address));
    }

    let time_field = field(&format!("Time ({})", beat.timezone), &beat.local_date_time);
    let (title, color) = if beat.up {
        let back_online = beat.heartbeat.time.timestamp();
        if let Some(last_down) = beat.heartbeat.last_down_time {
            let went_offline = last_down.timestamp();
            fields.push(field("Went Offline", &format!("<t:{went_offline}:F>")));
            fields.push(field(
                "Downtime Duration",
                &format_duration(back_online - went_offline),
            ));
        }
        fields.push(time_field);
        if let Some(ping) = beat.heartbeat.ping {
            fields.push(field("Ping", &format!("{ping} ms")));
        }
        (format!("✅ Your service {} is up! ✅", monitor.name), 65280)
    } else {
        let went_offline = beat.heartbeat.time.timestamp();
        fields.push(field("Went Offline", &format!("<t:{went_offline}:F>")));
        fields.push(time_field);
        fields.push(field("Error", beat.msg));
        (
            format!("❌ Your service {} went down. ❌", monitor.name),
            16711680,
        )
    };

    json!({
        "title": title,
        "color": color,
        "timestamp": notice.heartbeat_json.get("time"),
        "fields": fields,
    })
}

fn field(name: &str, value: &str) -> Value {
    json!({ "name": name, "value": value })
}

/// `formatDuration` (`discord.js:269-287`): `1h 2m`, `1m 35s`, seconds only below an hour.
fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    let mut parts = Vec::new();
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if secs > 0 && hours == 0 {
        parts.push(format!("{secs}s"));
    }
    if parts.is_empty() {
        "0s".to_owned()
    } else {
        parts.join(" ")
    }
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn formats_durations_like_kuma() {
        assert_eq!(format_duration(3725), "1h 2m");
        assert_eq!(format_duration(95), "1m 35s");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(0), "0s");
    }
}
