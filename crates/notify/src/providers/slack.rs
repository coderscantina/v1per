//! Kuma `slack.js`. The legacy `slackbutton` setting migration is the importer's job.

use reqwest::Url;
use serde_json::{Map, Value, json};

use crate::config::SlackConfig;
use crate::error::SendError;
use crate::event::{BeatNotice, Notice, extract_address};
use crate::{Delivered, SendContext, http, template};

pub(crate) async fn send(
    config: &SlackConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let url = http::secret_url(config.webhook_url.as_ref(), "Slack webhook URL")?;
    let payload = payload(config, notice)?;
    http::deliver(ctx.http.post(url).json(&payload)).await
}

fn payload(config: &SlackConfig, notice: &Notice<'_>) -> Result<Map<String, Value>, SendError> {
    let mut msg = notice.msg.clone();
    if config.channel_notify {
        msg.push_str(" <!channel>");
    }
    let base = |text: String| {
        let mut payload = Map::new();
        payload.insert("text".into(), text.into());
        for (key, value) in [
            ("channel", &config.channel),
            ("username", &config.username),
            ("icon_emoji", &config.icon_emoji),
        ] {
            if let Some(value) = value {
                payload.insert(key.into(), value.as_str().into());
            }
        }
        payload
    };

    let Some(beat) = &notice.beat else {
        return Ok(base(msg));
    };
    if config.use_template {
        let text = template::render(
            config.template.as_deref().unwrap_or_default(),
            &msg,
            &notice.monitor_json,
            &notice.heartbeat_json,
        )?;
        return Ok(base(text));
    }

    let include_group = config.include_group_name.unwrap_or(true);
    let group_path = if include_group {
        beat.group_path()
    } else {
        String::new()
    };
    let title = if beat.monitor.name.is_empty() {
        "Uptime Kuma Alert"
    } else {
        &beat.monitor.name
    };

    let mut payload = base(msg.clone());
    let mut attachments = Vec::new();
    if config.rich_message {
        attachments.push(json!({
            "color": if beat.up { "#2eb886" } else { "#e01e5a" },
            "blocks": blocks(notice, beat, title, &msg, &group_path),
        }));
    } else if group_path.is_empty() {
        payload.insert("text".into(), format!("{title}\n{msg}").into());
    } else {
        payload.insert(
            "text".into(),
            format!("_{group_path}_\n{title}\n{msg}").into(),
        );
    }
    payload.insert("attachments".into(), attachments.into());
    Ok(payload)
}

/// `buildBlocks` (`slack.js:84-135`).
fn blocks(
    notice: &Notice<'_>,
    beat: &BeatNotice<'_>,
    title: &str,
    msg: &str,
    group_path: &str,
) -> Vec<Value> {
    let mut blocks = vec![json!({
        "type": "header",
        "text": { "type": "plain_text", "text": title },
    })];
    if !group_path.is_empty() {
        blocks.push(json!({
            "type": "context",
            "elements": [{ "type": "mrkdwn", "text": format!("_{group_path}_") }],
        }));
    }
    blocks.push(json!({
        "type": "section",
        "fields": [
            { "type": "mrkdwn", "text": format!("*Message*\n{msg}") },
            {
                "type": "mrkdwn",
                "text": format!("*Time ({})*\n{}", beat.timezone, beat.local_date_time),
            },
        ],
    }));

    let mut actions = Vec::new();
    if let Some(link) = &beat.monitor.link {
        actions.push(button("Visit Uptime Kuma", "Uptime-Kuma", link));
    }
    let address = extract_address(&notice.monitor_json);
    if is_url(&address)
        && let Ok(url) = Url::parse(&address)
    {
        actions.push(button("Visit site", "Site", url.as_str()));
    }
    if !actions.is_empty() {
        blocks.push(json!({ "type": "actions", "elements": actions }));
    }
    blocks
}

fn button(text: &str, value: &str, url: &str) -> Value {
    json!({
        "type": "button",
        "text": { "type": "plain_text", "text": text },
        "value": value,
        "url": url,
    })
}

/// The `is-url` package Kuma uses: `//host.tld...` or `//localhost...`, optional scheme.
fn is_url(address: &str) -> bool {
    let rest = match address.split_once("//") {
        Some((scheme, rest))
            if scheme.is_empty()
                || scheme.strip_suffix(':').is_some_and(|name| {
                    !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                }) =>
        {
            rest
        }
        _ => return false,
    };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    if rest.starts_with("localhost") {
        return true;
    }
    match rest.find('.') {
        Some(dot) if dot > 0 => rest[dot + 1..].chars().count() >= 2,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_url;

    #[test]
    fn recognises_urls_like_is_url() {
        assert!(is_url("https://example.com/health"));
        assert!(is_url("http://localhost:8080"));
        assert!(!is_url("example.com"));
        assert!(!is_url("https://intranet"));
        assert!(!is_url("Heartbeat"));
    }
}
