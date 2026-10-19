//! Email, golden against Kuma 2.5.0 `server/notification-providers/smtp.js`, through a local
//! SMTP sink.

mod support;

use serde_json::{Value, json};
use support::events::{self, CERT_MSG, DOMAIN_MSG, PROBE_MSG};
use support::smtp::{Mail, Mode, SmtpSink};
use v1per_notify::{Event, SendContext, SendError, send};

/// The fixture templates from `crates/kuma-fixture`.
const SUBJECT: &str = "[{{ status }}] {{ name }}";
const BODY: &str = "<h1>{{ status }} {{ name }}</h1>\n<p>{{ msg }}</p>\n\
    {% if monitorJSON %}<p>Target: {{ hostnameOrURL }} ({{ monitorJSON.type }})</p>{% endif %}\n\
    {% if heartbeatJSON %}<p>{{ heartbeatJSON.localDateTime }} {{ heartbeatJSON.timezone }} \
    (UTC {{ heartbeatJSON.time }}), ping {{ heartbeatJSON.ping | default: \"n/a\" }}</p>{% endif %}";

/// A test-only key, generated for this file.
const DKIM_KEY: &str = "-----BEGIN PRIVATE KEY-----
MIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBALrgHnOc6CgAWBim
kyL3OwtjiLcRK7wmFWzSHjwklW9bAeUvOyeeOtaX5Gc/bgmfZWnYDe66rjbweG8Q
AlqE2sanom34uNsdGbB2703C8uHKc1MsucuU5S4GQLzwfT7nCt7MAHpQgHq/fLXE
DKYQl7aSoDo1AK0KMxfuVSvVTU5HAgMBAAECgYA940sybpxT4wlJ4KCJy+a59tSo
C/+358TgrRVmRZdXsYQHKoUksbo6p0L1DoSamzcyvmKe1SQs/aZk4aBl6empnxoP
vVYXtEVCvI81QWpc4lhE6HQ9DeVMAT9b89LoTobp6JIoqYiQyEXOhri6jwba9Vzd
KkBiWVcpSQCFTH3RgQJBAO70b8xQYj7TYeka/TOWTMRDi2tmL2/82x0SkTfSCv4Y
mIpZX0pWgtZRNxmrEeZ0HIuK+dXKEe8hRsYuR54uLa8CQQDINKiwofj7X5tvtJaR
cSw6fvUiUM76s11/FFlasvxouDzTlJirL5pLmsKNO4lQ55Ci39BIN99nAxDZJqDg
jmbpAkARB+z78/K8pZZB5gGL7rdz+ZhzRCo3Yh8/2DXvc5IDAD/HBI2tH7NdkRZL
Rw0UjswGCha6bReQIqAHuf6ywHnTAkBISwOS7JoMo0RsfMhV8ojnAosf/hAPZQ9g
4Qc3AiS057pHKIRMZF8R5+tStmnqRgPrXGhIe6x29RtzXkh/bcxpAkEAtnR1C5KK
eJkqyZUdXKnIuIh5cGkWYTjCwnu7Rq6Yl3lG+ByuVbV+rSmEQzl590WpfaqDYshI
c/UJRoZ8pGCXUg==
-----END PRIVATE KEY-----";

async fn deliver(sink: &SmtpSink, extra: Value, event: &Event) -> Result<(), SendError> {
    let mut config = json!({
        "type": "smtp",
        "smtpHost": "127.0.0.1",
        "smtpPort": sink.port,
        "smtpFrom": "\"Uptime\" <alerts@example.com>",
        "smtpTo": "ops@example.com",
    });
    config
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    send(&support::channel(config), event, &SendContext::new())
        .await
        .map(drop)
}

async fn mail(extra: Value, event: &Event) -> Mail {
    let sink = SmtpSink::start(Mode::Plain).await;
    deliver(&sink, extra, event).await.unwrap();
    sink.only_mail()
}

fn content_type(mail: &Mail) -> String {
    mail.header("Content-Type").unwrap()
}

// smtp.js:70-75: subject is `msg`, the body adds the local time line, plain text.
#[tokio::test]
async fn down_mail_matches_kuma() {
    let mail = mail(json!({}), &events::down()).await;
    let msg = "[API] [🔴 Down] timeout of 48000ms exceeded";
    assert_eq!(mail.header("Subject").unwrap(), msg);
    assert_eq!(
        mail.body(),
        format!("{msg}\nTime (Europe/Vienna): 2026-10-18 14:03:11")
    );
    assert!(content_type(&mail).starts_with("text/plain"));
}

#[tokio::test]
async fn up_mail_matches_kuma() {
    let mail = mail(json!({}), &events::up()).await;
    let msg = "[API] [✅ Up] 200 - OK";
    assert_eq!(mail.header("Subject").unwrap(), msg);
    assert_eq!(
        mail.body(),
        format!("{msg}\nTime (Europe/Vienna): 2026-10-18 15:05:16")
    );
}

// smtp.js:70-72: without a heartbeat, subject and body are both `msg`.
#[tokio::test]
async fn general_messages_use_msg_for_subject_and_body() {
    for (event, msg) in [
        (events::certificate(), CERT_MSG),
        (events::domain(), DOMAIN_MSG),
        (events::probe_offline(), PROBE_MSG),
        (events::test("Email"), "Email Testing"),
    ] {
        let mail = mail(json!({}), &event).await;
        assert_eq!(mail.header("Subject").unwrap(), msg);
        assert_eq!(mail.body(), msg);
    }
}

// smtp.js:77-89: custom subject and body render; `htmlBody` sends the body as HTML.
#[tokio::test]
async fn custom_templates_render_up_and_down() {
    let extra = || json!({ "customSubject": SUBJECT, "customBody": BODY, "htmlBody": true });

    let down = mail(extra(), &events::down()).await;
    assert_eq!(down.header("Subject").unwrap(), "[🔴 Down] API");
    assert_eq!(
        down.body(),
        "<h1>🔴 Down API</h1>\n<p>[API] [🔴 Down] timeout of 48000ms exceeded</p>\n\
         <p>Target: https://api.example.com/health (http)</p>\n\
         <p>2026-10-18 14:03:11 Europe/Vienna (UTC 2026-10-18 12:03:11.123), ping n/a</p>"
    );
    assert!(content_type(&down).starts_with("text/html"));

    let up = mail(extra(), &events::up()).await;
    assert_eq!(up.header("Subject").unwrap(), "[✅ Up] API");
    assert!(
        up.body()
            .ends_with("(UTC 2026-10-18 13:05:16.456), ping 123</p>")
    );
}

// smtp.js:77: certificate and domain mails ignore templates; the test mail uses them.
#[tokio::test]
async fn custom_templates_skip_expiry_mails_but_apply_to_tests() {
    let extra = || json!({ "customSubject": SUBJECT, "customBody": BODY, "htmlBody": true });

    for (event, msg) in [
        (events::certificate(), CERT_MSG),
        (events::domain(), DOMAIN_MSG),
    ] {
        let mail = mail(extra(), &event).await;
        assert_eq!(mail.header("Subject").unwrap(), msg);
        assert_eq!(mail.body(), msg);
        assert!(content_type(&mail).starts_with("text/plain"));
    }

    let test = mail(extra(), &events::test("Email")).await;
    assert_eq!(
        test.header("Subject").unwrap(),
        "[⚠️ Test] Monitor Name not available"
    );
    assert!(
        test.body()
            .starts_with("<h1>⚠️ Test Monitor Name not available</h1>\n<p>Email Testing</p>\n")
    );
}

// smtp.js:84-87: `htmlBody` only applies to a custom body.
#[tokio::test]
async fn html_flag_without_custom_body_stays_text() {
    let mail = mail(json!({ "htmlBody": true }), &events::down()).await;
    assert!(content_type(&mail).starts_with("text/plain"));
}

// smtp.js:51-68, :93-102: credentials, recipients, extra headers; Bcc stays off the headers.
#[tokio::test]
async fn sends_to_every_recipient_with_extra_headers() {
    let extra = json!({
        "smtpTo": "ops@example.com, oncall@example.org",
        "smtpCC": "lead@example.com",
        "smtpBCC": "audit@example.net",
        "smtpUsername": "alerts@example.com",
        "smtpPassword": "hunter2",
        "smtpAdditionalHeaders": "{\"X-Fixture\": \"kuma\"}",
    });
    let mail = mail(extra, &events::down()).await;

    assert_eq!(mail.mail_from, "alerts@example.com");
    assert_eq!(
        mail.rcpt_to,
        [
            "ops@example.com",
            "oncall@example.org",
            "lead@example.com",
            "audit@example.net"
        ]
    );
    assert_eq!(
        mail.auth,
        Some(("alerts@example.com".into(), "hunter2".into()))
    );
    // nodemailer quotes the display name, lettre only when needed.
    assert_eq!(mail.header("From").unwrap(), "Uptime <alerts@example.com>");
    assert_eq!(mail.header("Cc").unwrap(), "lead@example.com");
    assert_eq!(mail.header("X-Fixture").unwrap(), "kuma");
    assert_eq!(mail.header("Bcc"), None);
}

// smtp.js:17, :33-35: implicit TLS, accepting the sink's self-signed certificate.
#[tokio::test]
async fn implicit_tls_accepts_untrusted_cert_when_told_to() {
    let sink = SmtpSink::start(Mode::ImplicitTls).await;
    let extra = json!({ "smtpSecure": true, "smtpIgnoreTLSError": true });
    deliver(&sink, extra, &events::down()).await.unwrap();
    assert!(sink.only_mail().tls);
}

#[tokio::test]
async fn untrusted_cert_fails_by_default() {
    let sink = SmtpSink::start(Mode::ImplicitTls).await;
    let error = deliver(&sink, json!({ "smtpSecure": true }), &events::down())
        .await
        .unwrap_err();
    assert!(matches!(error, SendError::Smtp { .. }), "{error:?}");
    assert!(sink.mails().is_empty());
}

// smtp.js:22-36: without `smtpSecure` the connection upgrades when offered, unless disabled.
#[tokio::test]
async fn starttls_is_used_when_offered_unless_disabled() {
    let sink = SmtpSink::start(Mode::StartTls).await;
    deliver(
        &sink,
        json!({ "smtpIgnoreTLSError": true }),
        &events::down(),
    )
    .await
    .unwrap();
    assert!(sink.only_mail().tls);

    let sink = SmtpSink::start(Mode::StartTls).await;
    deliver(
        &sink,
        json!({ "smtpIgnoreSTARTTLS": true }),
        &events::down(),
    )
    .await
    .unwrap();
    assert!(!sink.only_mail().tls);
}

#[tokio::test]
async fn server_rejections_are_classified() {
    let sink = SmtpSink::rejecting_rcpt(Mode::Plain, Some("550 5.1.1 unknown user")).await;
    let error = deliver(&sink, json!({}), &events::down())
        .await
        .unwrap_err();
    assert!(!error.is_retryable(), "{error:?}");

    let sink = SmtpSink::rejecting_rcpt(Mode::Plain, Some("451 4.3.0 try later")).await;
    let error = deliver(&sink, json!({}), &events::down())
        .await
        .unwrap_err();
    assert!(error.is_retryable(), "{error:?}");
}

// smtp.js:39-48: DKIM with the configured domain, selector and header list.
#[tokio::test]
async fn signs_with_dkim_when_a_domain_is_set() {
    let extra = json!({
        "smtpDkimDomain": "example.com",
        "smtpDkimKeySelector": "fixture",
        "smtpDkimPrivateKey": DKIM_KEY,
        "smtpDkimHashAlgo": "sha256",
        "smtpDkimheaderFieldNames": "From:To:Subject",
    });
    let signature = mail(extra, &events::down())
        .await
        .header("DKIM-Signature")
        .unwrap();
    for part in [
        "a=rsa-sha256",
        "d=example.com",
        "s=fixture",
        "h=from:to:subject",
    ] {
        assert!(signature.contains(part), "{part} missing in {signature}");
    }
}
