//! Discord payloads, golden against Kuma 2.5.0 `server/notification-providers/discord.js`.

mod support;

use serde_json::{Value, json};
use support::events::{self, CERT_MSG, DOMAIN_MSG, DOWN_UNIX, PROBE_MSG, UP_UNIX};
use support::http::{FakeHttp, Reply};
use v1per_notify::{Event, SendContext, send};

/// The fixture template from `crates/kuma-fixture`.
const TEMPLATE: &str = "{{ status }} **{{ name }}** ({{ hostnameOrURL }})\n{{ msg }}\
    {% if heartbeatJSON %}\nSince {{ heartbeatJSON.localDateTime }} {{ heartbeatJSON.timezone }}{% endif %}";

async fn post(fake: &FakeHttp, extra: Value, event: &Event) -> Value {
    let mut config = json!({ "type": "discord", "discordWebhookUrl": format!("{}/api/webhooks/1/token", fake.url) });
    config
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    send(&support::channel(config), event, &SendContext::new())
        .await
        .unwrap();
    fake.posted().json()
}

async fn default_payload(event: &Event) -> Value {
    post(&FakeHttp::start().await, json!({}), event).await
}

async fn custom_payload(event: &Event) -> Value {
    let extra = json!({ "discordMessageFormat": "custom", "discordMessageTemplate": TEMPLATE });
    post(&FakeHttp::start().await, extra, event).await
}

// discord.js:122-176: title, colour, raw `time` as timestamp, fields in order.
#[tokio::test]
async fn down_embed_matches_kuma() {
    assert_eq!(
        default_payload(&events::down()).await,
        json!({
            "username": "Uptime Kuma",
            "embeds": [{
                "title": "❌ Your service API went down. ❌",
                "color": 16711680,
                "timestamp": "2026-10-18 12:03:11.123",
                "fields": [
                    { "name": "Service Name", "value": "API" },
                    { "name": "Service URL", "value": "https://api.example.com/health" },
                    { "name": "Went Offline", "value": format!("<t:{DOWN_UNIX}:F>") },
                    { "name": "Time (Europe/Vienna)", "value": "2026-10-18 14:03:11" },
                    { "name": "Error", "value": "timeout of 48000ms exceeded" },
                ],
            }],
        })
    );
}

// discord.js:177-257: downtime from `lastDownTime`, `1h 2m` drops seconds, ping last.
#[tokio::test]
async fn up_embed_matches_kuma() {
    assert_eq!(
        default_payload(&events::up()).await,
        json!({
            "username": "Uptime Kuma",
            "embeds": [{
                "title": "✅ Your service API is up! ✅",
                "color": 65280,
                "timestamp": "2026-10-18 13:05:16.456",
                "fields": [
                    { "name": "Service Name", "value": "API" },
                    { "name": "Service URL", "value": "https://api.example.com/health" },
                    { "name": "Went Offline", "value": format!("<t:{DOWN_UNIX}:F>") },
                    { "name": "Downtime Duration", "value": "1h 2m" },
                    { "name": "Time (Europe/Vienna)", "value": "2026-10-18 15:05:16" },
                    { "name": "Ping", "value": "123 ms" },
                ],
            }],
        })
    );
    assert_eq!(UP_UNIX - DOWN_UNIX, 3725);
}

// discord.js:42-67: messages without a heartbeat are plain content in every default config.
#[tokio::test]
async fn general_messages_are_plain_content() {
    for (event, msg) in [
        (events::certificate(), CERT_MSG),
        (events::domain(), DOMAIN_MSG),
        (events::probe_offline(), PROBE_MSG),
        (events::test("Discord"), "Discord Testing"),
    ] {
        assert_eq!(
            default_payload(&event).await,
            json!({ "username": "Uptime Kuma", "content": msg })
        );
    }
}

// discord.js:96-120: a custom template replaces the embed with rendered content.
#[tokio::test]
async fn custom_template_renders_up_and_down() {
    assert_eq!(
        custom_payload(&events::down()).await,
        json!({
            "username": "Uptime Kuma",
            "content": "🔴 Down **API** (https://api.example.com/health)\n\
                [API] [🔴 Down] timeout of 48000ms exceeded\n\
                Since 2026-10-18 14:03:11 Europe/Vienna",
        })
    );
    assert_eq!(
        custom_payload(&events::up()).await["content"],
        "✅ Up **API** (https://api.example.com/health)\n\
            [API] [✅ Up] 200 - OK\nSince 2026-10-18 15:05:16 Europe/Vienna"
    );
}

// notification-provider.js:84-96: without objects the template sees Kuma's test placeholders.
#[tokio::test]
async fn custom_template_renders_general_messages_with_placeholders() {
    for (event, msg) in [
        (events::certificate(), CERT_MSG),
        (events::domain(), DOMAIN_MSG),
        (events::probe_offline(), PROBE_MSG),
    ] {
        assert_eq!(
            custom_payload(&event).await["content"],
            format!("⚠️ Test **Monitor Name not available** (testing.hostname)\n{msg}")
        );
    }
}

// discord.js:38-39: the legacy flag selects custom only when no format is stored; a stored
// template without either stays on embeds.
#[tokio::test]
async fn legacy_template_flag_selects_custom_format() {
    let fake = FakeHttp::start().await;
    let extra = json!({ "discordUseMessageTemplate": true, "discordMessageTemplate": "{{ name }} {{ status }}" });
    assert_eq!(
        post(&fake, extra, &events::down()).await["content"],
        "API 🔴 Down"
    );

    let fake = FakeHttp::start().await;
    let extra = json!({ "discordMessageTemplate": "{{ name }}" });
    assert!(post(&fake, extra, &events::down()).await["embeds"].is_array());
}

// discord.js:73-93 and :42-49.
#[tokio::test]
async fn minimalist_format_is_one_line() {
    let extra = || json!({ "discordMessageFormat": "minimalist" });
    let down = post(&FakeHttp::start().await, extra(), &events::down()).await;
    let up = post(&FakeHttp::start().await, extra(), &events::up()).await;
    let cert = post(&FakeHttp::start().await, extra(), &events::certificate()).await;
    assert_eq!(down["content"], "🔴 API is down.");
    assert_eq!(up["content"], "🟢 API is up.");
    assert_eq!(cert["content"], format!("Test: {CERT_MSG}"));
}

// discord.js:18-26, :59-64, :137, :168-173: username, prefix, hidden URL, thread, flags.
#[tokio::test]
async fn channel_options_shape_the_request() {
    let fake = FakeHttp::start().await;
    let extra = json!({
        "discordUsername": "Uptime",
        "discordPrefixMessage": "@here",
        "discordChannelType": "postToThread",
        "threadId": "100000000000000099",
        "disableUrl": true,
        "discordSuppressNotifications": true,
    });
    let payload = post(&fake, extra, &events::down()).await;

    assert_eq!(payload["username"], "Uptime");
    assert_eq!(payload["content"], "@here");
    assert_eq!(payload["flags"], 4096);
    let names: Vec<_> = payload["embeds"][0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        [
            "Service Name",
            "Went Offline",
            "Time (Europe/Vienna)",
            "Error"
        ]
    );
    for request in fake.requests() {
        assert_eq!(
            request.target,
            "/api/webhooks/1/token?thread_id=100000000000000099"
        );
    }

    let fake = FakeHttp::start().await;
    let extra = json!({ "discordChannelType": "createNewForumPost", "postName": "Outage" });
    assert_eq!(
        post(&fake, extra, &events::down()).await["thread_name"],
        "Outage"
    );
}

// discord.js:28-36: Kuma's icon only when the webhook has no avatar of its own.
#[tokio::test]
async fn avatar_falls_back_to_kuma_icon() {
    let fake = FakeHttp::with(Reply {
        avatar: None,
        ..Reply::default()
    })
    .await;
    let payload = post(&fake, json!({}), &events::up()).await;
    assert_eq!(
        payload["avatar_url"],
        "https://github.com/louislam/uptime-kuma/raw/master/public/icon.png"
    );
    assert_eq!(fake.requests()[0].method, "GET");
}
