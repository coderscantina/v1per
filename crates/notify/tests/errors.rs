//! Config parsing, failure classification and secret hygiene.

mod support;

use std::time::Duration;

use serde_json::json;
use support::events;
use support::http::{FakeHttp, Reply};
use v1per_notify::{ChannelConfig, SendContext, SendError, check_template, send};

fn webhook(url: &str) -> ChannelConfig {
    support::channel(json!({ "type": "webhook", "webhookURL": url }))
}

#[tokio::test]
async fn http_status_decides_retry() {
    for (status, retryable) in [(500, true), (429, true), (400, false), (404, false)] {
        let fake = FakeHttp::with(Reply {
            status,
            body: "nope",
            ..Reply::default()
        })
        .await;
        let error = send(&webhook(&fake.url), &events::down(), &SendContext::new())
            .await
            .unwrap_err();
        assert!(
            matches!(&error, SendError::Http { status: s, body } if *s == status && body == "nope")
        );
        assert_eq!(error.is_retryable(), retryable, "HTTP {status}");
    }
}

#[tokio::test]
async fn slow_providers_hit_the_hard_timeout() {
    let fake = FakeHttp::with(Reply {
        delay: Duration::from_secs(5),
        ..Reply::default()
    })
    .await;
    let ctx = SendContext::new().with_timeout(Duration::from_millis(200));
    let error = send(&webhook(&fake.url), &events::down(), &ctx)
        .await
        .unwrap_err();
    assert!(matches!(error, SendError::Timeout(_)));
    assert!(error.is_retryable());
}

#[tokio::test]
async fn connection_errors_are_retryable_and_hide_the_url() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config = support::channel(json!({
        "type": "discord",
        "discordWebhookUrl": format!("http://127.0.0.1:{port}/api/webhooks/1/secret-token"),
    }));
    let error = send(&config, &events::down(), &SendContext::new())
        .await
        .unwrap_err();
    assert!(error.is_retryable(), "{error:?}");
    assert!(!error.to_string().contains("secret-token"), "{error}");
    assert!(!format!("{error:?}").contains("secret-token"));

    let error = send(
        &webhook("not a url secret-token"),
        &events::down(),
        &SendContext::new(),
    )
    .await
    .unwrap_err();
    assert!(!error.is_retryable());
    assert!(!error.to_string().contains("secret-token"), "{error}");
}

#[test]
fn debug_output_hides_secrets() {
    let configs = [
        json!({ "type": "smtp", "smtpPassword": "S1", "smtpDkimPrivateKey": "S2", "smtpAdditionalHeaders": "{\"X\":\"S3\"}" }),
        json!({ "type": "discord", "discordWebhookUrl": "https://discord.com/api/webhooks/1/S1" }),
        json!({ "type": "webhook", "webhookURL": "https://h/S1", "webhookAdditionalHeaders": "{\"A\":\"S2\"}" }),
        json!({ "type": "slack", "slackwebhookURL": "https://hooks.slack.com/S1" }),
        json!({ "type": "teams", "webhookUrl": "https://teams/S1" }),
        json!({ "type": "telegram", "telegramBotToken": "S1" }),
        json!({ "type": "ntfy", "ntfypassword": "S1", "ntfyaccesstoken": "S2" }),
        json!({ "type": "pushover", "pushoveruserkey": "S1", "pushoverapptoken": "S2" }),
        json!({ "type": "gotify", "gotifyapplicationToken": "S1" }),
    ];
    for config in configs {
        let debug = format!("{:?}", support::channel(config));
        for secret in ["S1", "S2", "S3"] {
            assert!(!debug.contains(secret), "{secret} leaked in {debug}");
        }
    }
}

// Kuma stores form values as typed by the Vue inputs; numbers arrive as strings too.
#[test]
fn reads_kuma_configs_with_loose_types() {
    let config = support::channel(json!({
        "type": "smtp", "name": "Email", "isDefault": true, "applyExisting": false,
        "smtpHost": "mail.example.org", "smtpPort": "465", "smtpSecure": 1, "smtpCC": "",
        "smtpIgnoreTLSError": null, "unknownField": { "kept": "out" },
    }));
    let ChannelConfig::Smtp(smtp) = config else {
        panic!("expected smtp");
    };
    assert_eq!(smtp.port, Some(465));
    assert!(smtp.secure);
    assert!(!smtp.ignore_tls_error);
    assert_eq!(smtp.cc, None);

    let ChannelConfig::Pushover(pushover) = support::channel(
        json!({ "type": "pushover", "pushoverpriority": "-1", "pushoverttl": 3600, "pushoverdevice": "" }),
    ) else {
        panic!("expected pushover");
    };
    assert_eq!(pushover.priority, Some(-1));
    assert_eq!(pushover.ttl, Some(3600));
    assert_eq!(pushover.device, None);
}

#[tokio::test]
async fn unsupported_providers_keep_their_config() {
    let config = support::channel(json!({
        "type": "gotify", "gotifyserverurl": "https://gotify.example.org", "gotifyapplicationToken": "t0k",
    }));
    let ChannelConfig::Unsupported(unsupported) = &config else {
        panic!("expected unsupported");
    };
    assert_eq!(unsupported.kind(), "gotify");
    assert_eq!(unsupported.raw()["gotifyapplicationToken"], "t0k");

    let error = send(&config, &events::down(), &SendContext::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SendError::Unsupported(ref kind) if kind == "gotify"));
    assert!(!error.is_retryable());
}

#[tokio::test]
async fn broken_templates_fail_permanently() {
    let template = "{{ msg | where_exp: 'x', 'x' }}";
    assert!(check_template(template).is_err());
    assert!(check_template("{{ msg | upcase }}").is_ok());

    let fake = FakeHttp::start().await;
    let config = support::channel(json!({
        "type": "discord", "discordWebhookUrl": fake.url,
        "discordMessageFormat": "custom", "discordMessageTemplate": template,
    }));
    let error = send(&config, &events::down(), &SendContext::new())
        .await
        .unwrap_err();
    assert!(matches!(error, SendError::Template(_)), "{error:?}");
    assert!(!error.is_retryable());
    assert!(fake.requests().is_empty());
}
