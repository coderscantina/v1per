//! Kuma `teams.js`: an Adaptive Card through a Teams workflow webhook.

use serde_json::{Value, json};

use crate::config::TeamsConfig;
use crate::error::SendError;
use crate::event::{BeatNotice, Notice, extract_address};
use crate::{Delivered, SendContext, http};

const LOGO_URL: &str =
    "https://raw.githubusercontent.com/louislam/uptime-kuma/master/public/icon.png";

pub(crate) async fn send(
    config: &TeamsConfig,
    notice: &Notice<'_>,
    ctx: &SendContext,
) -> Result<Delivered, SendError> {
    let url = http::secret_url(config.webhook_url.as_ref(), "Teams webhook URL")?;
    let payload = payload(config, notice);
    http::deliver(ctx.http.post(url).json(&payload)).await
}

/// `_notificationPayloadFactory` (`teams.js:62-212`). Messages without a beat get a card
/// with only the description, like Kuma's `_handleGeneralNotification`.
fn payload(config: &TeamsConfig, notice: &Notice<'_>) -> Value {
    let beat = notice.beat.as_ref();
    let mut facts = Vec::new();
    let mut actions = Vec::new();

    if let Some(link) = beat.and_then(|beat| beat.monitor.link.as_ref()) {
        actions.push(open_url("Visit Uptime Kuma", link));
    }
    let description = beat.map_or(notice.msg.as_str(), |beat| beat.msg);
    if !description.is_empty() {
        facts.push(fact("Description", description));
    }
    if let Some(beat) = beat {
        if !beat.monitor.name.is_empty() {
            facts.push(fact("Monitor", &beat.monitor.name));
        }
        let address = extract_address(&notice.monitor_json);
        if !address.is_empty() && address != "https://" {
            facts.push(fact("URL", &format!("[{address}]({address})")));
            actions.push(open_url("Visit Monitor URL", &address));
        }
        facts.push(fact(
            "Time",
            &format!("{} ({})", beat.local_date_time, beat.timezone),
        ));
    }

    let mut body = vec![
        json!({
            "type": "Container",
            "verticalContentAlignment": "Center",
            "items": [{
                "type": "ColumnSet",
                "style": style(beat),
                "columns": [
                    {
                        "type": "Column",
                        "width": "auto",
                        "verticalContentAlignment": "Center",
                        "items": [{
                            "type": "Image",
                            "width": "32px",
                            "style": "Person",
                            "url": LOGO_URL,
                            "altText": "Uptime Kuma Logo",
                        }],
                    },
                    {
                        "type": "Column",
                        "width": "stretch",
                        "items": [
                            {
                                "type": "TextBlock",
                                "size": "Medium",
                                "weight": "Bolder",
                                "text": format!("**{}**", status_message(beat, false)),
                            },
                            {
                                "type": "TextBlock",
                                "size": "Small",
                                "weight": "Default",
                                "text": "Uptime Kuma Alert",
                                "isSubtle": true,
                                "spacing": "None",
                            },
                        ],
                    },
                ],
            }],
        }),
        json!({ "type": "FactSet", "separator": false, "facts": facts }),
    ];

    if let Some(beat) = beat
        && config.enable_tags
        && !beat.monitor.tags.is_empty()
    {
        let badges: Vec<Value> = beat
            .monitor
            .tags
            .iter()
            .map(|tag| {
                let text = if tag.value.is_empty() {
                    tag.name.clone()
                } else {
                    format!("{}: {}", tag.name, tag.value)
                };
                json!({ "type": "Badge", "text": text, "size": "Medium", "style": "Accent" })
            })
            .collect();
        body.push(json!({
            "type": "Container",
            "layouts": [{
                "type": "Layout.Flow",
                "columnSpacing": "Small",
                "rowSpacing": "Small",
                "horizontalItemsAlignment": "Left",
            }],
            "items": badges,
        }));
    }
    // Kuma appends the action set even when it is empty.
    body.push(json!({ "type": "ActionSet", "actions": actions }));

    json!({
        "type": "message",
        "summary": status_message(beat, true),
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "contentUrl": "",
            "content": {
                "type": "AdaptiveCard",
                "body": body,
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "version": "1.5",
            },
        }],
    })
}

fn status_message(beat: Option<&BeatNotice<'_>>, with_symbol: bool) -> String {
    match beat {
        Some(beat) if beat.up => {
            let symbol = if with_symbol { "✅ " } else { "" };
            format!("{symbol}[{}] is back online", beat.monitor.name)
        }
        Some(beat) => {
            let symbol = if with_symbol { "🔴 " } else { "" };
            format!("{symbol}[{}] went down", beat.monitor.name)
        }
        None => "Notification".to_owned(),
    }
}

fn style(beat: Option<&BeatNotice<'_>>) -> &'static str {
    match beat {
        Some(beat) if beat.up => "good",
        Some(_) => "attention",
        None => "emphasis",
    }
}

fn fact(title: &str, value: &str) -> Value {
    json!({ "title": title, "value": value })
}

fn open_url(title: &str, url: &str) -> Value {
    json!({ "type": "Action.OpenUrl", "title": title, "url": url })
}
