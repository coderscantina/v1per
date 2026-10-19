//! Webhook, Slack, Teams, Telegram, ntfy and Pushover requests, against Kuma 2.5.0's providers.

mod support;

use serde_json::{Value, json};
use support::events::{self, CERT_MSG};
use support::http::{Captured, FakeHttp};
use v1per_notify::{Event, SendContext, send};

async fn capture(config: Value, event: &Event) -> Captured {
    let fake = FakeHttp::start().await;
    let config = serde_json::to_string(&config)
        .unwrap()
        .replace("FAKE", &fake.url);
    let ctx = SendContext::new().with_pushover_endpoint(format!("{}/1/messages.json", fake.url));
    send(
        &support::channel(serde_json::from_str(&config).unwrap()),
        event,
        &ctx,
    )
    .await
    .unwrap();
    fake.posted()
}

// webhook.js:17-21, :63: `{heartbeat, monitor, msg}` with Kuma's key order.
#[tokio::test]
async fn webhook_posts_kuma_json_body() {
    let request = capture(
        json!({ "type": "webhook", "webhookURL": "FAKE/hook", "webhookContentType": "json" }),
        &events::down(),
    )
    .await;
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert!(request.text().starts_with(
        "{\"heartbeat\":{\"monitorID\":7,\"status\":0,\"time\":\"2026-10-18 12:03:11.123\",\
         \"msg\":\"timeout of 48000ms exceeded\",\"ping\":null,\"important\":true,\"duration\":60,\
         \"retries\":0,\"response\":null,\"timezone\":\"Europe/Vienna\",\"timezoneOffset\":\"+02:00\",\
         \"localDateTime\":\"2026-10-18 14:03:11\"},\"monitor\":{\"id\":7,\"name\":\"API\",\
         \"path\":[\"Production\",\"API\"],\"pathName\":\"Production / API\""
    ));
    let body = request.json();
    assert_eq!(body["msg"], "[API] [🔴 Down] timeout of 48000ms exceeded");
    assert_eq!(body["monitor"]["tags"][1]["value"], "frankfurt");
}

// webhook.js:43-56: the rendered string as a form body; extra headers win.
#[tokio::test]
async fn webhook_custom_body_renders_with_extra_headers() {
    let config = json!({
        "type": "webhook", "webhookURL": "FAKE/hook", "webhookContentType": "custom",
        "webhookCustomBody": "{\"text\": \"{{ name }}: {{ msg }}\", \"status\": \"{{ status }}\"}",
        "webhookAdditionalHeaders": "{\"Authorization\": \"Bearer fixture-webhook-secret\"}",
    });
    let request = capture(config, &events::up()).await;
    assert_eq!(
        request.text(),
        "{\"text\": \"API: [API] [✅ Up] 200 - OK\", \"status\": \"✅ Up\"}"
    );
    assert_eq!(
        request.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        request.header("authorization"),
        Some("Bearer fixture-webhook-secret")
    );

    let config = json!({
        "type": "webhook", "webhookURL": "FAKE/hook", "webhookContentType": "custom",
        "webhookCustomBody": "{{ msg | json }}",
        "webhookAdditionalHeaders": "{\"Content-Type\": \"application/json\"}",
    });
    let request = capture(config, &events::domain()).await;
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(
        request.json(),
        json!("Domain name example.com will expire in 20 days")
    );
}

// webhook.js:38-42: one multipart field `data` with the JSON body.
#[tokio::test]
async fn webhook_form_data_sends_one_data_field() {
    let config =
        json!({ "type": "webhook", "webhookURL": "FAKE/hook", "webhookContentType": "form-data" });
    let request = capture(config, &events::certificate()).await;
    assert!(
        request
            .header("content-type")
            .unwrap()
            .starts_with("multipart/form-data")
    );
    let text = request.text();
    assert!(text.contains("name=\"data\""));
    assert!(text.contains(&format!(
        "{{\"heartbeat\":null,\"monitor\":null,\"msg\":\"{CERT_MSG}\"}}"
    )));
}

// webhook.js:26-37: GET carries `msg` and, when present, the objects as JSON strings.
#[tokio::test]
async fn webhook_get_sends_query_parameters() {
    let fake = FakeHttp::start().await;
    let config = support::channel(
        json!({ "type": "webhook", "webhookURL": format!("{}/hook", fake.url), "httpMethod": "GET" }),
    );
    send(&config, &events::domain(), &SendContext::new())
        .await
        .unwrap();
    send(&config, &events::down(), &SendContext::new())
        .await
        .unwrap();
    let requests = fake.requests();
    assert_eq!(
        requests[0].target,
        "/hook?msg=Domain+name+example.com+will+expire+in+20+days"
    );
    let query: Vec<(String, String)> =
        url::form_urlencoded::parse(requests[1].target.split_once('?').unwrap().1.as_bytes())
            .into_owned()
            .collect();
    let keys: Vec<_> = query.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["msg", "heartbeat", "monitor"]);
    let monitor: Value = serde_json::from_str(&query[2].1).unwrap();
    assert_eq!(monitor["name"], "API");
}

// slack.js:36-135, :143-145, :182-208: rich message with group context and both buttons.
#[tokio::test]
async fn slack_rich_message_matches_kuma() {
    let config = json!({
        "type": "slack", "slackwebhookURL": "FAKE/services/T/B/x", "slackchannel": "#alerts",
        "slackusername": "uptime", "slackiconemo": ":robot_face:", "slackchannelnotify": true,
        "slackrichmessage": true,
    });
    let msg = "[API] [🔴 Down] timeout of 48000ms exceeded <!channel>";
    assert_eq!(
        capture(config, &events::down()).await.json(),
        json!({
            "text": msg,
            "channel": "#alerts",
            "username": "uptime",
            "icon_emoji": ":robot_face:",
            "attachments": [{
                "color": "#e01e5a",
                "blocks": [
                    { "type": "header", "text": { "type": "plain_text", "text": "API" } },
                    { "type": "context", "elements": [{ "type": "mrkdwn", "text": "_Production_" }] },
                    { "type": "section", "fields": [
                        { "type": "mrkdwn", "text": format!("*Message*\n{msg}") },
                        { "type": "mrkdwn", "text": "*Time (Europe/Vienna)*\n2026-10-18 14:03:11" },
                    ] },
                    { "type": "actions", "elements": [
                        { "type": "button", "text": { "type": "plain_text", "text": "Visit Uptime Kuma" },
                          "value": "Uptime-Kuma", "url": "https://v1per.example.com/monitors/7" },
                        { "type": "button", "text": { "type": "plain_text", "text": "Visit site" },
                          "value": "Site", "url": "https://api.example.com/health" },
                    ] },
                ],
            }],
        })
    );
}

// slack.js:202-207: plain text puts the group path and name above the message.
#[tokio::test]
async fn slack_plain_message_includes_group_path() {
    let config = json!({ "type": "slack", "slackwebhookURL": "FAKE/s" });
    let body = capture(config, &events::up()).await.json();
    assert_eq!(body["text"], "_Production_\nAPI\n[API] [✅ Up] 200 - OK");
    assert_eq!(body["attachments"], json!([]));
}

// teams.js:62-212: summary, style, facts, tag badges and actions.
#[tokio::test]
async fn teams_card_matches_kuma() {
    let config =
        json!({ "type": "teams", "webhookUrl": "FAKE/webhookb2/x", "teamsEnableTags": true });
    let body = capture(config, &events::down()).await.json();
    assert_eq!(body["summary"], "🔴 [API] went down");
    let card = &body["attachments"][0]["content"];
    assert_eq!(card["body"][0]["items"][0]["style"], "attention");
    assert_eq!(
        card["body"][0]["items"][0]["columns"][1]["items"][0]["text"],
        "**[API] went down**"
    );
    assert_eq!(
        card["body"][1]["facts"],
        json!([
            { "title": "Description", "value": "timeout of 48000ms exceeded" },
            { "title": "Monitor", "value": "API" },
            { "title": "URL", "value": "[https://api.example.com/health](https://api.example.com/health)" },
            { "title": "Time", "value": "2026-10-18 14:03:11 (Europe/Vienna)" },
        ])
    );
    let badges: Vec<_> = card["body"][2]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["text"].as_str().unwrap())
        .collect();
    assert_eq!(badges, ["prod", "region: frankfurt"]);
    assert_eq!(card["body"][3]["type"], "ActionSet");
    assert_eq!(card["body"][3]["actions"][0]["title"], "Visit Uptime Kuma");
    assert_eq!(card["body"][3]["actions"][1]["title"], "Visit Monitor URL");
}

// telegram.js:56-104: token in the path, MarkdownV2 escapes every value.
#[tokio::test]
async fn telegram_template_escapes_markdown_v2() {
    let config = json!({
        "type": "telegram", "telegramBotToken": "123:token", "telegramChatID": -1001234567890_i64,
        "telegramServerUrl": "FAKE", "telegramProtectContent": true, "telegramUseTemplate": true,
        "telegramTemplate": "*{{ status }}* {{ monitorJSON.url }} {{ msg }}",
        "telegramTemplateParseMode": "MarkdownV2",
    });
    let request = capture(config, &events::up()).await;
    assert_eq!(request.target, "/bot123:token/sendMessage");
    assert_eq!(
        request.json(),
        json!({
            "chat_id": "-1001234567890",
            "text": "*✅ Up* https://api\\.example\\.com/health \\[API\\] \\[✅ Up\\] 200 \\- OK",
            "disable_notification": false,
            "protect_content": true,
            "link_preview_options": { "is_disabled": true },
            "parse_mode": "MarkdownV2",
        })
    );
}

// ntfy.js:15-29, :63-129: bearer token, raised down priority, tags, view action, icon.
#[tokio::test]
async fn ntfy_down_matches_kuma() {
    let config = json!({
        "type": "ntfy", "ntfyserverurl": "FAKE", "ntfytopic": "uptime", "ntfyPriority": 3,
        "ntfyAuthenticationMethod": "accessToken", "ntfyaccesstoken": "tk_secret",
        "ntfyIcon": "https://uptime.example.com/icon.png",
    });
    let request = capture(config, &events::down()).await;
    assert_eq!(request.header("authorization"), Some("Bearer tk_secret"));
    assert_eq!(
        request.json(),
        json!({
            "topic": "uptime",
            "message": "timeout of 48000ms exceeded",
            "priority": 4,
            "title": "API Down [Uptime-Kuma]",
            "tags": ["red_circle", "prod", "region: frankfurt"],
            "actions": [{ "action": "view", "label": "Open API", "url": "https://api.example.com/health" }],
            "icon": "https://uptime.example.com/icon.png",
        })
    );
}

// pushover.js:16-56: fixed retry/expire, up sound, link, time line in HTML.
#[tokio::test]
async fn pushover_up_matches_kuma() {
    let config = json!({
        "type": "pushover", "pushoveruserkey": "ukey", "pushoverapptoken": "atoken",
        "pushoversounds": "siren", "pushoversounds_up": "magic", "pushoverpriority": "1",
        "pushovertitle": "Uptime", "pushoverdevice": "", "pushoverttl": "3600",
    });
    let request = capture(config, &events::up()).await;
    assert_eq!(request.target, "/1/messages.json");
    assert_eq!(
        request.json(),
        json!({
            "message": "[API] [✅ Up] 200 - OK\n<b>Time (Europe/Vienna)</b>: 2026-10-18 15:05:16",
            "user": "ukey",
            "token": "atoken",
            "sound": "magic",
            "priority": 1,
            "title": "Uptime",
            "retry": "30",
            "expire": "3600",
            "html": 1,
            "url": "https://v1per.example.com/monitors/7",
            "url_title": "Link to Monitor",
            "ttl": 3600,
        })
    );
}
