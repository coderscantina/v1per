//! Kuma `ntfy.js`: JSON publishing with the topic in the body.

use serde_json::{Map, Value, json};

use crate::config::{NtfyAuth, NtfyConfig};
use crate::error::SendError;
use crate::event::Notice;
use crate::providers::trimmed;
use crate::{Delivered, SendContext, http, template};

pub(crate) async fn send(
    config: &NtfyConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let url = http::url(config.server_url.as_deref(), "ntfy server URL")?;
    let payload = payload(config, notice)?;

    let mut request = ctx.http.post(url);
    match config.auth {
        Some(NtfyAuth::UsernamePassword) => {
            request = request.basic_auth(
                config.username.as_deref().unwrap_or_default(),
                Some(config.password.as_ref().map_or("", |p| p.expose())),
            );
        }
        Some(NtfyAuth::AccessToken) => {
            let token = config
                .access_token
                .as_ref()
                .ok_or_else(|| SendError::Config("ntfy access token is missing".into()))?;
            request = request.bearer_auth(token.expose());
        }
        Some(NtfyAuth::None) | None => {}
    }
    if let Some(call) = &config.call {
        request = request.header("X-Call", call);
    }
    http::deliver(request.json(&payload)).await
}

fn payload(config: &NtfyConfig, notice: &Notice<'_>) -> Result<Map<String, Value>, SendError> {
    let topic = config
        .topic
        .as_deref()
        .ok_or_else(|| SendError::Config("ntfy topic is missing".into()))?;
    let templated = |default: String, template: Option<&str>| match trimmed(template) {
        Some(template) if config.use_template => template::render(
            template,
            &notice.msg,
            &notice.monitor_json,
            &notice.heartbeat_json,
        ),
        _ => Ok(default),
    };

    let mut payload = Map::new();
    payload.insert("topic".into(), topic.into());

    let Some(beat) = &notice.beat else {
        let title = templated(
            format!("{topic} [Uptime-Kuma]"),
            config.custom_title.as_deref(),
        )?;
        let message = templated(notice.msg.clone(), config.custom_message.as_deref())?;
        payload.insert("title".into(), title.into());
        payload.insert("message".into(), message.into());
        if let Some(priority) = config.priority {
            payload.insert("priority".into(), priority.into());
        }
        payload.insert("tags".into(), json!(["test_tube"]));
        return Ok(payload);
    };

    // Kuma's `||`: an unset or zero priority means 4, down defaults to one level higher.
    let priority = config.priority.filter(|p| *p != 0).unwrap_or(4);
    let (status, priority, mut tags) = if beat.up {
        ("Up", priority, vec!["green_circle".to_owned()])
    } else {
        let down = config
            .priority_down
            .filter(|p| *p != 0)
            .unwrap_or(if priority == 5 {
                5
            } else {
                priority.saturating_add(1)
            });
        ("Down", down, vec!["red_circle".to_owned()])
    };
    tags.extend(beat.monitor.tags.iter().map(|tag| {
        if tag.value.is_empty() {
            tag.name.clone()
        } else {
            format!("{}: {}", tag.name, tag.value)
        }
    }));
    let name = &beat.monitor.name;
    let title = templated(
        format!("{name} {status} [Uptime-Kuma]"),
        config.custom_title.as_deref(),
    )?;
    let message = templated(beat.msg.to_owned(), config.custom_message.as_deref())?;

    payload.insert("message".into(), message.into());
    payload.insert("priority".into(), priority.into());
    payload.insert("title".into(), title.into());
    payload.insert("tags".into(), tags.into());
    if let Some(url) = beat.monitor.url.as_deref().filter(|url| *url != "https://") {
        payload.insert(
            "actions".into(),
            json!([{ "action": "view", "label": format!("Open {name}"), "url": url }]),
        );
    }
    if let Some(icon) = &config.icon {
        payload.insert("icon".into(), icon.as_str().into());
    }
    Ok(payload)
}
