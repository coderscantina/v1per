//! Kuma `pushover.js`. No templates.

use serde_json::{Map, Value};

use crate::config::PushoverConfig;
use crate::error::SendError;
use crate::event::Notice;
use crate::{Delivered, SendContext, http};

pub(crate) const ENDPOINT: &str = "https://api.pushover.net/1/messages.json";

pub(crate) async fn send(
    config: &PushoverConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let url = http::url(Some(&ctx.pushover_endpoint), "Pushover endpoint")?;
    let payload = payload(config, notice);
    http::deliver(ctx.http.post(url).json(&payload)).await
}

fn payload(config: &PushoverConfig, notice: &Notice<'_>) -> Map<String, Value> {
    let mut message = notice.msg.clone();
    let mut sound = config.sound.as_deref();
    if let Some(beat) = &notice.beat {
        // Non-up events keep the down sound, as in Kuma.
        if beat.up && config.sound_up.is_some() {
            sound = config.sound_up.as_deref();
        }
        message.push_str(&format!(
            "\n<b>Time ({})</b>: {}",
            beat.timezone, beat.local_date_time
        ));
    }

    let mut payload = Map::new();
    payload.insert("message".into(), message.into());
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            payload.insert(key.into(), value);
        }
    };
    put("user", config.user_key.as_ref().map(|s| s.expose().into()));
    put(
        "token",
        config.app_token.as_ref().map(|s| s.expose().into()),
    );
    put("sound", sound.map(Into::into));
    put("priority", config.priority.map(Into::into));
    put("title", config.title.as_deref().map(Into::into));
    put("retry", Some("30".into()));
    put("expire", Some("3600".into()));
    put("html", Some(1.into()));
    let link = notice
        .beat
        .as_ref()
        .and_then(|beat| beat.monitor.link.as_deref());
    put("url", link.map(Into::into));
    put("url_title", link.map(|_| "Link to Monitor".into()));
    put("device", config.device.as_deref().map(Into::into));
    put("ttl", config.ttl.map(Into::into));
    payload
}
