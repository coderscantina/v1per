//! Configuration rows: everything except heartbeats and `stat_*` rollups.
//!
//! Hostnames are `example.com`, `example.org` or `example.net`, addresses come from the
//! documentation ranges and every secret contains "fixture". IDs follow insertion order.

use std::collections::HashMap;

use chrono::{
    DateTime, Datelike, Duration, FixedOffset, NaiveTime, TimeZone, Timelike, Utc, Weekday,
};
use rusqlite::{Connection, types::Value};
use serde_json::json;

use crate::history::{Outages, Plan};
use crate::{
    API_KEYS, HISTORY_DAYS, KEEP_DATA_PERIOD_DAYS, Result, STATUS_PAGE_PASSWORD, USER_NAME,
    USER_PASSWORD, USER_TOTP_SECRET, migration_names, now,
};

/// A monitor as the history generator needs it.
pub(crate) struct Monitor {
    pub id: i64,
    pub name: &'static str,
    pub kind: &'static str,
    pub interval: i64,
    pub retry_interval: i64,
    pub maxretries: i64,
    pub resend_interval: i64,
    pub upside_down: bool,
    pub plan: Plan,
}

trait Sql {
    fn sql(self) -> Value;
}

impl Sql for Value {
    fn sql(self) -> Value {
        self
    }
}
impl Sql for &str {
    fn sql(self) -> Value {
        Value::Text(self.to_owned())
    }
}
impl Sql for String {
    fn sql(self) -> Value {
        Value::Text(self)
    }
}
impl Sql for i64 {
    fn sql(self) -> Value {
        Value::Integer(self)
    }
}
impl Sql for i32 {
    fn sql(self) -> Value {
        Value::Integer(self.into())
    }
}
impl Sql for bool {
    fn sql(self) -> Value {
        Value::Integer(self.into())
    }
}
impl Sql for f64 {
    fn sql(self) -> Value {
        Value::Real(self)
    }
}
impl<T: Sql> Sql for Option<T> {
    fn sql(self) -> Value {
        self.map_or(Value::Null, Sql::sql)
    }
}

type Row = Vec<(&'static str, Value)>;

/// A monitor on a status page: name, `send_url`, `custom_url`.
type PageMember = (&'static str, bool, Option<&'static str>);

macro_rules! row {
    ($($key:literal => $value:expr),* $(,)?) => {
        vec![$(($key, Sql::sql($value))),*]
    };
}

fn put(conn: &Connection, table: &str, row: Row) -> Result<i64> {
    let columns = row
        .iter()
        .map(|(name, _)| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let placeholders = vec!["?"; row.len()].join(", ");
    let sql = format!("INSERT INTO \"{table}\" ({columns}) VALUES ({placeholders})");
    conn.prepare_cached(&sql)?
        .execute(rusqlite::params_from_iter(
            row.into_iter().map(|(_, value)| value),
        ))?;
    Ok(conn.last_insert_rowid())
}

/// `YYYY-MM-DD HH:MM:SS`, the format of Kuma's `DATETIME('now')` defaults.
fn ts(time: DateTime<Utc>) -> String {
    time.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn bcrypt_hash(secret: &str, salt: u8) -> Result<String> {
    // Kuma uses bcryptjs with cost 10, which writes the `$2a$` prefix.
    Ok(bcrypt::hash_with_salt(secret, 10, [salt; 16])?.format_for_version(bcrypt::Version::TwoA))
}

pub(crate) fn insert(conn: &Connection) -> Result<Vec<Monitor>> {
    let start = now() - Duration::days(HISTORY_DAYS);

    put(
        conn,
        "knex_migrations_lock",
        row! { "index" => 1, "is_locked" => 0 },
    )?;
    for (i, name) in migration_names().into_iter().enumerate() {
        let applied = start + Duration::milliseconds(i as i64 * 37);
        put(
            conn,
            "knex_migrations",
            row! {
                "name" => name, "batch" => 1, "migration_time" => applied.timestamp_millis(),
            },
        )?;
    }

    settings(conn)?;

    put(
        conn,
        "user",
        row! {
            "username" => USER_NAME,
            "password" => bcrypt_hash(USER_PASSWORD, 0x11)?,
            "active" => true,
            "timezone" => "auto",
            "twofa_secret" => USER_TOTP_SECRET,
            "twofa_status" => true,
            "twofa_last_token" => "123456",
        },
    )?;

    for (i, (name, key)) in [
        ("CI deploy hook", API_KEYS[0]),
        ("Retired dashboard", API_KEYS[1]),
    ]
    .into_iter()
    .enumerate()
    {
        put(
            conn,
            "api_key",
            row! {
                "key" => bcrypt_hash(key, 0x21 + i as u8)?,
                "name" => name,
                "user_id" => 1,
                "created_date" => ts(start + Duration::days(10 + i as i64 * 100)),
                "active" => i == 0,
                "expires" => (i == 1).then(|| ts(now() - Duration::days(30))),
            },
        )?;
    }

    put(
        conn,
        "docker_host",
        row! {
            "user_id" => 1, "docker_daemon" => "/var/run/docker.sock", "docker_type" => "socket", "name" => "Local Docker",
        },
    )?;
    put(
        conn,
        "docker_host",
        row! {
            "user_id" => 1, "docker_daemon" => "tcp://docker.example.com:2376", "docker_type" => "tcp", "name" => "Build host",
        },
    )?;
    put(
        conn,
        "remote_browser",
        row! {
            "name" => "Browserless",
            "url" => "ws://browser.example.com:3000/chromium?token=fixture-browser-token",
            "user_id" => 1,
        },
    )?;
    put(
        conn,
        "proxy",
        row! {
            "user_id" => 1, "protocol" => "http", "host" => "proxy.example.com", "port" => 3128, "auth" => true,
            "username" => "fixture", "password" => "fixture-proxy-password", "active" => true, "default" => false,
            "created_date" => ts(start + Duration::days(1)),
        },
    )?;
    put(
        conn,
        "proxy",
        row! {
            "user_id" => 1, "protocol" => "socks5", "host" => "socks.example.org", "port" => 1080, "auth" => false,
            "username" => Value::Null, "password" => Value::Null, "active" => true, "default" => false,
            "created_date" => ts(start + Duration::days(2)),
        },
    )?;

    for (name, color) in [
        ("production", "#059669"),
        ("eu", "#2563EB"),
        ("critical", "#DC2626"),
        ("team-web", "#7C3AED"),
    ] {
        put(
            conn,
            "tag",
            row! { "name" => name, "color" => color, "created_date" => ts(start) },
        )?;
    }

    let notifications = notifications(conn)?;
    let monitors = monitors(conn, start)?;
    let id = |name: &str| {
        monitors
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.id)
            .expect("monitor in catalog")
    };

    for (monitor, channels) in [
        (
            "API",
            vec![
                "Email (custom template)",
                "Discord (custom template)",
                "Pushover",
            ],
        ),
        ("Homepage", vec!["Discord (thread)", "Email (default)"]),
        ("Shop keyword", vec!["Webhook", "Slack"]),
        ("Nightly backup", vec!["Telegram", "Email (default)"]),
        ("Gateway ping", vec!["ntfy", "Microsoft Teams"]),
        ("Production", vec!["Discord (custom template)"]),
        ("Status JSON", vec!["Gotify", "Matrix", "Mattermost"]),
    ] {
        for channel in channels {
            put(
                conn,
                "monitor_notification",
                row! {
                    "monitor_id" => id(monitor), "notification_id" => notifications[channel],
                },
            )?;
        }
    }

    for (monitor, tag, value) in [
        ("API", 1, ""),
        ("API", 2, "frankfurt"),
        ("API", 3, ""),
        ("Homepage", 1, ""),
        ("Homepage", 4, ""),
        ("Shop keyword", 4, "checkout"),
        ("Nightly backup", 3, "nightly"),
        ("Production", 1, ""),
    ] {
        put(
            conn,
            "monitor_tag",
            row! { "monitor_id" => id(monitor), "tag_id" => tag, "value" => value },
        )?;
    }

    tls_and_expiry(conn, &id)?;
    status_pages(conn, start, &id)?;
    maintenance(conn, start, &id)?;

    Ok(monitors)
}

fn settings(conn: &Connection) -> Result<()> {
    // Values are JSON encoded, as `Settings.set` does. `jwtSecret` and `database_version`
    // are written raw by their own code paths, and have no type.
    let patched: serde_json::Map<String, serde_json::Value> = LEGACY_PATCHES
        .iter()
        .map(|name| ((*name).to_owned(), serde_json::Value::Bool(true)))
        .collect();
    let rows: [(&str, String, Option<&str>); 17] = [
        ("database_version", "10".into(), None),
        (
            "databasePatchedFiles",
            serde_json::Value::Object(patched).to_string(),
            None,
        ),
        (
            "migrateAggregateTableState",
            json!("migrated").to_string(),
            None,
        ),
        ("jwtSecret", bcrypt_hash("fixture-jwt-secret", 0x31)?, None),
        ("initServerTimezone", "true".into(), None),
        (
            "serverTimezone",
            json!("Asia/Kolkata").to_string(),
            Some("general"),
        ),
        (
            "keepDataPeriodDays",
            json!(KEEP_DATA_PERIOD_DAYS).to_string(),
            Some("general"),
        ),
        (
            "primaryBaseURL",
            json!("https://uptime.example.com").to_string(),
            Some("general"),
        ),
        ("entryPage", json!("dashboard").to_string(), Some("general")),
        ("searchEngineIndex", "false".into(), Some("general")),
        ("checkUpdate", "false".into(), Some("general")),
        ("checkBeta", "false".into(), Some("general")),
        ("dnsCache", "false".into(), Some("general")),
        ("nscd", "true".into(), Some("general")),
        ("trustProxy", "true".into(), Some("general")),
        ("tlsExpiryNotifyDays", "[7,14,21]".into(), Some("general")),
        (
            "domainExpiryNotifyDays",
            "[7,14,21]".into(),
            Some("general"),
        ),
    ];
    for (key, value, kind) in rows {
        put(
            conn,
            "setting",
            row! { "key" => key, "value" => value, "type" => kind },
        )?;
    }
    Ok(())
}

/// `Database.patchList` of Kuma 2.5.0 (`server/database.js:71-116`). The parent of
/// `patch-add-other-auth.sql` is listed before it, as the recursion records it.
const LEGACY_PATCHES: [&str; 44] = [
    "patch-setting-value-type.sql",
    "patch-improve-performance.sql",
    "patch-2fa.sql",
    "patch-add-retry-interval-monitor.sql",
    "patch-incident-table.sql",
    "patch-group-table.sql",
    "patch-monitor-push_token.sql",
    "patch-http-monitor-method-body-and-headers.sql",
    "patch-2fa-invalidate-used-token.sql",
    "patch-notification_sent_history.sql",
    "patch-monitor-basic-auth.sql",
    "patch-add-docker-columns.sql",
    "patch-status-page.sql",
    "patch-proxy.sql",
    "patch-monitor-expiry-notification.sql",
    "patch-status-page-footer-css.sql",
    "patch-added-mqtt-monitor.sql",
    "patch-add-clickable-status-page-link.sql",
    "patch-add-sqlserver-monitor.sql",
    "patch-add-other-auth.sql",
    "patch-grpc-monitor.sql",
    "patch-add-radius-monitor.sql",
    "patch-monitor-add-resend-interval.sql",
    "patch-ping-packet-size.sql",
    "patch-maintenance-table2.sql",
    "patch-add-gamedig-monitor.sql",
    "patch-add-google-analytics-status-page-tag.sql",
    "patch-http-body-encoding.sql",
    "patch-add-description-monitor.sql",
    "patch-api-key-table.sql",
    "patch-monitor-tls.sql",
    "patch-maintenance-cron.sql",
    "patch-add-parent-monitor.sql",
    "patch-add-invert-keyword.sql",
    "patch-added-json-query.sql",
    "patch-added-kafka-producer.sql",
    "patch-add-certificate-expiry-status-page.sql",
    "patch-monitor-oauth-cc.sql",
    "patch-add-timeout-monitor.sql",
    "patch-add-gamedig-given-port.sql",
    "patch-notification-config.sql",
    "patch-fix-kafka-producer-booleans.sql",
    "patch-timeout.sql",
    "patch-monitor-tls-info-add-fk.sql",
];

fn notifications(conn: &Connection) -> Result<HashMap<&'static str, i64>> {
    let email_body = "<h1>{{ status }} {{ name }}</h1>\n\
        <p>{{ msg }}</p>\n\
        {% if monitorJSON %}<p>Target: {{ hostnameOrURL }} ({{ monitorJSON.type }})</p>{% endif %}\n\
        {% if heartbeatJSON %}<p>{{ heartbeatJSON.localDateTime }} {{ heartbeatJSON.timezone }} \
        (UTC {{ heartbeatJSON.time }}), ping {{ heartbeatJSON.ping | default: \"n/a\" }}</p>{% endif %}";
    let discord_template = "{{ status }} **{{ name }}** ({{ hostnameOrURL }})\n{{ msg }}\
        {% if heartbeatJSON %}\nSince {{ heartbeatJSON.localDateTime }} {{ heartbeatJSON.timezone }}{% endif %}";

    let configs = [
        (
            "Email (custom template)",
            false,
            json!({
                "type": "smtp", "smtpHost": "smtp.example.com", "smtpPort": 587, "smtpSecure": false,
                "smtpIgnoreTLSError": false, "smtpIgnoreSTARTTLS": false, "smtpUsername": "alerts@example.com",
                "smtpPassword": "fixture-smtp-password", "smtpFrom": "\"Uptime\" <alerts@example.com>",
                "smtpTo": "ops@example.com, oncall@example.org", "smtpCC": "lead@example.com", "smtpBCC": "audit@example.net",
                "customSubject": "[{{ status }}] {{ name }}", "customBody": email_body, "htmlBody": true,
                "smtpAdditionalHeaders": "{\"X-Fixture\": \"kuma\"}",
                "smtpDkimDomain": "example.com", "smtpDkimKeySelector": "fixture",
                "smtpDkimPrivateKey": "-----BEGIN PRIVATE KEY-----\nFIXTURE-NOT-A-REAL-KEY\n-----END PRIVATE KEY-----",
                "smtpDkimHashAlgo": "sha256", "smtpDkimheaderFieldNames": "From:To:Subject", "smtpDkimskipFields": "",
            }),
        ),
        (
            "Email (default)",
            true,
            json!({
                "type": "smtp", "smtpHost": "mail.example.org", "smtpPort": 465, "smtpSecure": true,
                "smtpUsername": "fixture", "smtpPassword": "fixture-smtp-password-2",
                "smtpFrom": "uptime@example.org", "smtpTo": "team@example.org",
            }),
        ),
        (
            "Discord (custom template)",
            false,
            json!({
                "type": "discord", "discordWebhookUrl": "https://discord.example.com/api/webhooks/100000000000000001/fixture-webhook-token",
                "discordUsername": "Uptime", "discordMessageFormat": "custom", "discordMessageTemplate": discord_template,
                "discordChannelType": "channel", "disableUrl": false, "discordSuppressNotifications": false,
            }),
        ),
        (
            "Discord (thread)",
            false,
            json!({
                "type": "discord", "discordWebhookUrl": "https://discord.example.com/api/webhooks/100000000000000002/fixture-webhook-token-2",
                "discordMessageFormat": "normal", "discordPrefixMessage": "@here", "discordChannelType": "postToThread",
                "threadId": "100000000000000099", "disableUrl": true, "discordSuppressNotifications": true,
            }),
        ),
        (
            "Webhook",
            false,
            json!({
                "type": "webhook", "webhookURL": "https://hooks.example.com/uptime", "httpMethod": "post",
                "webhookContentType": "custom",
                "webhookCustomBody": "{\"text\": \"{{ name }}: {{ msg }}\", \"status\": \"{{ status }}\"}",
                "webhookAdditionalHeaders": "{\"Authorization\": \"Bearer fixture-webhook-secret\"}",
            }),
        ),
        (
            "Slack",
            false,
            json!({
                "type": "slack", "slackwebhookURL": "https://hooks.slack.example.com/services/T000/B000/fixture",
                "slackchannel": "#alerts", "slackusername": "uptime", "slackiconemo": ":robot_face:",
                "slackchannelnotify": true, "slackrichmessage": true, "slackIncludeGroupName": true, "slackUseTemplate": false,
            }),
        ),
        (
            "Microsoft Teams",
            false,
            json!({
                "type": "teams", "webhookUrl": "https://teams.example.com/webhookb2/fixture", "teamsEnableTags": false,
            }),
        ),
        (
            "Telegram",
            false,
            json!({
                "type": "telegram", "telegramBotToken": "123456789:FIXTURE-not-a-real-bot-token",
                "telegramChatID": "-1001234567890", "telegramMessageThreadID": "", "telegramServerUrl": "https://api.telegram.org",
                "telegramSendSilently": false, "telegramProtectContent": true, "telegramUseTemplate": true,
                "telegramTemplate": "<b>{{ status }}</b> {{ name }}\n{{ msg }}", "telegramTemplateParseMode": "HTML",
            }),
        ),
        (
            "ntfy",
            false,
            json!({
                "type": "ntfy", "ntfyserverurl": "https://ntfy.example.com", "ntfytopic": "uptime-fixture",
                "ntfyPriority": 4, "ntfyPriorityDown": 5, "ntfyAuthenticationMethod": "accessToken",
                "ntfyaccesstoken": "tk_fixture_not_a_real_token", "ntfyIcon": "https://uptime.example.com/icon.png",
            }),
        ),
        (
            "Pushover",
            false,
            json!({
                "type": "pushover", "pushoveruserkey": "ufixture000000000000000000000000", "pushoverapptoken": "afixture000000000000000000000000",
                "pushoversounds": "siren", "pushoversounds_up": "magic", "pushoverpriority": "1", "pushovertitle": "Uptime",
                "pushoverdevice": "", "pushoverttl": "3600",
            }),
        ),
        (
            "Gotify",
            false,
            json!({
                "type": "gotify", "gotifyserverurl": "https://gotify.example.org", "gotifyapplicationToken": "fixture-gotify-token",
                "gotifyPriority": 8,
            }),
        ),
        (
            "Matrix",
            false,
            json!({
                "type": "matrix", "homeserverUrl": "https://matrix.example.org", "internalRoomId": "!fixture:example.org",
                "accessToken": "fixture-matrix-token",
            }),
        ),
        (
            "Mattermost",
            false,
            json!({
                "type": "mattermost", "mattermostWebhookUrl": "https://chat.example.net/hooks/fixture",
                "mattermostusername": "uptime", "mattermostchannel": "ops", "mattermosticonemo": ":rotating_light:",
                "mattermosticonurl": "",
            }),
        ),
    ];

    let mut ids = HashMap::new();
    for (name, is_default, mut config) in configs {
        let object = config.as_object_mut().expect("config is an object");
        object.insert("name".into(), name.into());
        object.insert("isDefault".into(), is_default.into());
        object.insert("applyExisting".into(), false.into());
        let id = put(
            conn,
            "notification",
            row! {
                "name" => name, "active" => true, "user_id" => 1, "is_default" => is_default, "config" => config.to_string(),
            },
        )?;
        ids.insert(name, id);
    }
    Ok(ids)
}

/// Builder for one monitor row.
struct Def {
    name: &'static str,
    kind: &'static str,
    parent: Option<&'static str>,
    interval: i64,
    maxretries: i64,
    retry_interval: i64,
    resend_interval: i64,
    upside_down: bool,
    active: bool,
    columns: Row,
    plan: Plan,
}

fn def(name: &'static str, kind: &'static str, parent: Option<&'static str>, interval: i64) -> Def {
    Def {
        name,
        kind,
        parent,
        interval,
        maxretries: 1,
        retry_interval: interval,
        resend_interval: 0,
        upside_down: false,
        active: true,
        columns: Vec::new(),
        plan: Plan::recent(),
    }
}

impl Def {
    fn columns(mut self, columns: Row) -> Self {
        self.columns.extend(columns);
        self
    }
    fn retries(mut self, maxretries: i64, retry_interval: i64) -> Self {
        self.maxretries = maxretries;
        self.retry_interval = retry_interval;
        self
    }
    fn resend(mut self, beats: i64) -> Self {
        self.resend_interval = beats;
        self
    }
    fn upside_down(mut self) -> Self {
        self.upside_down = true;
        self
    }
    fn paused(mut self) -> Self {
        self.active = false;
        self.plan = Plan::Nothing;
        self
    }
    fn plan(mut self, plan: Plan) -> Self {
        self.plan = plan;
        self
    }
}

pub(crate) fn up_message(kind: &str) -> &'static str {
    match kind {
        "http" | "keyword" | "json-query" | "real-browser" | "globalping" => "200 - OK",
        "ping" | "port" | "tailscale-ping" => "",
        "group" => "All children up and running",
        "push" => "OK",
        "manual" => "Up",
        "docker" => "healthy",
        "dns" => "Records: 192.0.2.10",
        "websocket-upgrade" => "101 - Switching Protocols",
        _ => "OK",
    }
}

fn monitor_defs() -> Vec<Def> {
    const WEB: Option<&str> = Some("Web");
    const EDGE: Option<&str> = Some("Edge");
    const EU: Option<&str> = Some("EU");
    const STAGING: Option<&str> = Some("Staging");
    const DB: &str = "fixture-db-password";
    let day = 24;
    let oracle_down_ms = 70 * 60 * 1000;

    vec![
        def("Production", "group", None, 60),
        def("EU", "group", Some("Production"), 60),
        def("Web", "group", EU, 60),
        def("Edge", "group", WEB, 60),
        def("Staging", "group", None, 60).plan(Plan::checked(
            6,
            Outages::Since { ms_before_now: oracle_down_ms, message: "Child monitors down: Oracle" },
        )),
        def("Empty group", "group", None, 60).plan(Plan::constant(2, "Group empty")),
        def("API", "http", EU, 300)
            .retries(2, 60)
            .resend(3)
            .columns(row! {
                "url" => "https://api.example.com/health", "method" => "GET",
                "headers" => "{\"Accept\": \"application/json\"}", "auth_method" => "basic",
                "basic_auth_user" => "fixture", "basic_auth_pass" => "fixture-basic-password", "proxy_id" => 1,
                "expiry_notification" => true, "domain_expiry_notification" => true, "save_error_response" => true,
                "description" => "Public API health endpoint",
            })
            .plan(Plan::history(HISTORY_DAYS * day, 0.12, 120, api_in_maintenance)),
        def("Homepage", "http", WEB, 60).columns(row! {
            "url" => "https://www.example.com", "accepted_statuscodes_json" => "[\"200-299\",\"301\"]",
            "maxredirects" => 5, "cache_bust" => true, "expiry_notification" => true, "ip_family" => "ipv4",
        }),
        def("Login (OAuth)", "http", EU, 120).columns(row! {
            "url" => "https://login.example.com/api/me", "auth_method" => "oauth2-cc",
            "oauth_client_id" => "fixture-client", "oauth_client_secret" => "fixture-client-secret",
            "oauth_token_url" => "https://login.example.com/oauth/token", "oauth_scopes" => "read",
            "oauth_auth_method" => "client_secret_basic", "oauth_audience" => "https://api.example.com",
        }),
        def("Admin (mTLS)", "http", EU, 120).columns(row! {
            "url" => "https://admin.example.com", "auth_method" => "mtls",
            "tls_cert" => "-----BEGIN CERTIFICATE-----\nFIXTURE\n-----END CERTIFICATE-----",
            "tls_key" => "-----BEGIN PRIVATE KEY-----\nFIXTURE-NOT-A-REAL-KEY\n-----END PRIVATE KEY-----",
            "tls_ca" => "-----BEGIN CERTIFICATE-----\nFIXTURE-CA\n-----END CERTIFICATE-----",
        }),
        def("Bearer API", "http", EU, 120).columns(row! {
            "url" => "https://api.example.org/v2/status", "method" => "POST", "body" => "{\"probe\": true}",
            "http_body_encoding" => "json", "auth_method" => "bearer", "bearer_token" => "fixture-bearer-token",
            "save_response" => true, "response_max_length" => 2048,
        }),
        def("Old intranet (NTLM)", "http", STAGING, 300).columns(row! {
            "url" => "https://intranet.example.net", "auth_method" => "ntlm", "basic_auth_user" => "fixture",
            "basic_auth_pass" => "fixture-ntlm-password", "auth_domain" => "EXAMPLE", "auth_workstation" => "PROBE01",
            "proxy_id" => 2, "ignore_tls" => true,
        }),
        def("Should stay down", "http", STAGING, 120)
            .upside_down()
            .columns(row! { "url" => "https://decommissioned.example.com" })
            .plan(Plan::checked(6, Outages::Always { message: "getaddrinfo ENOTFOUND decommissioned.example.com" })),
        def("Paused http", "http", STAGING, 60).paused().columns(row! { "url" => "https://paused.example.com" }),
        def("Shop keyword", "keyword", WEB, 600)
            .retries(1, 120)
            .columns(row! { "url" => "https://shop.example.com", "keyword" => "Add to cart", "invert_keyword" => false })
            .plan(Plan::history(HISTORY_DAYS * day, 0.05, 340, shop_in_maintenance)),
        def("No maintenance banner", "keyword", WEB, 300)
            .columns(row! { "url" => "https://www.example.com", "keyword" => "maintenance", "invert_keyword" => true }),
        def("Status JSON", "json-query", EU, 60).columns(row! {
            "url" => "https://api.example.com/status.json", "json_path" => "$.status", "json_path_operator" => "==",
            "expected_value" => "ok", "retry_only_on_status_code_failure" => true,
        }),
        def("SSH", "port", EDGE, 60).columns(row! { "hostname" => "ssh.example.com", "port" => 22 }),
        def("Gateway ping", "ping", EDGE, 60)
            .columns(row! {
                "hostname" => "gw.example.net", "packet_size" => 56, "ping_count" => 3, "ping_numeric" => true,
                "ping_per_request_timeout" => 2,
            })
            .plan(Plan::history(2 * day, 0.5, 11, gateway_in_maintenance)),
        def("DNS A record", "dns", EDGE, 300).columns(row! {
            "hostname" => "example.com", "port" => 53, "dns_resolve_type" => "A", "dns_resolve_server" => "192.0.2.53",
            "dns_last_result" => "192.0.2.10",
        }),
        def("Docker app", "docker", STAGING, 60).columns(row! { "docker_container" => "app", "docker_host" => 1 }),
        def("Docker build", "docker", STAGING, 300).columns(row! { "docker_container" => "builder", "docker_host" => 2 }),
        def("nginx service", "system-service", STAGING, 60).columns(row! { "system_service_name" => "nginx" }),
        def("PM2 worker", "pm2", STAGING, 60).columns(row! { "system_service_name" => "worker" }),
        def("Checkout browser", "real-browser", WEB, 600).columns(row! {
            "url" => "https://shop.example.com/checkout", "remote_browser" => 1, "screenshot_delay" => 2,
        }),
        def("Nightly backup", "push", Some("Production"), 3600)
            .retries(1, 300)
            .resend(6)
            .columns(row! { "push_token" => "fixturePushToken0000000000000001" })
            .plan(Plan::Push { hours: HISTORY_DAYS * day }),
        def("Batch job", "push", STAGING, 600).columns(row! { "push_token" => "fixturePushToken0000000000000002" }),
        def("Manual status", "manual", None, 60).columns(row! { "manual_status" => 1 }),
        def("Manual unset", "manual", None, 60).plan(Plan::constant(2, "Manual monitoring - No status set")),
        def("Globalping HTTP", "globalping", EU, 600).columns(row! {
            "url" => "https://www.example.com", "subtype" => "http", "location" => "Europe", "protocol" => "HTTPS",
            "ping_count" => 3,
        }),
        def("gRPC health", "grpc-keyword", EU, 60).columns(row! {
            "grpc_url" => "grpc.example.com:443", "grpc_protobuf" => "syntax = \"proto3\";",
            "grpc_service_name" => "grpc.health.v1.Health", "grpc_method" => "Check", "grpc_body" => "{}",
            "grpc_metadata" => "{}", "grpc_enable_tls" => true, "keyword" => "SERVING",
        }),
        def("Kafka producer", "kafka-producer", STAGING, 120).columns(row! {
            "kafka_producer_topic" => "uptime", "kafka_producer_brokers" => "[\"kafka.example.com:9092\"]",
            "kafka_producer_sasl_options" => "{\"mechanism\":\"plain\",\"username\":\"fixture\",\"password\":\"fixture-kafka-password\"}",
            "kafka_producer_message" => "ping", "kafka_producer_ssl" => true, "kafka_producer_allow_auto_topic_creation" => false,
        }),
        def("MQTT sensor", "mqtt", STAGING, 60).columns(row! {
            "hostname" => "mqtt.example.com", "port" => 1883, "mqtt_topic" => "sensors/door", "mqtt_username" => "fixture",
            "mqtt_password" => "fixture-mqtt-password", "mqtt_check_type" => "keyword", "mqtt_success_message" => "closed",
            "mqtt_websocket_path" => Value::Null,
        }),
        def("NTP", "ntp", EDGE, 300).columns(row! { "hostname" => "time.example.com", "port" => 123 }),
        def("RabbitMQ", "rabbitmq", STAGING, 60).columns(row! {
            "rabbitmq_nodes" => "[\"https://rabbit.example.com:15672\"]", "rabbitmq_username" => "fixture",
            "rabbitmq_password" => "fixture-rabbit-password",
        }),
        def("SIP", "sip-options", EDGE, 300).columns(row! { "hostname" => "sip.example.com", "port" => 5060 }),
        def("Mail server", "smtp", EDGE, 300)
            .columns(row! { "hostname" => "mail.example.com", "port" => 587, "smtp_security" => "starttls" }),
        def("Switch SNMP", "snmp", EDGE, 300).columns(row! {
            "hostname" => "switch.example.net", "port" => 161, "snmp_oid" => "1.3.6.1.2.1.1.3.0", "snmp_version" => "2c",
            "radius_password" => "fixture-community", "json_path" => "$", "json_path_operator" => ">", "expected_value" => "0",
        }),
        def("Tailscale node", "tailscale-ping", EDGE, 120).columns(row! { "hostname" => "node.example.com" }),
        def("WebSocket", "websocket-upgrade", WEB, 120).columns(row! {
            "url" => "wss://ws.example.com/socket", "accepted_statuscodes_json" => "[\"1000\"]", "ws_subprotocol" => "",
        }),
        def("SQL Server", "sqlserver", STAGING, 300).columns(row! {
            "database_connection_string" => format!("Server=sql.example.com,1433;Database=app;User Id=fixture;Password={DB};Encrypt=true"),
            "database_query" => "SELECT 1",
        }),
        def("MongoDB", "mongodb", STAGING, 300).columns(row! {
            "database_connection_string" => format!("mongodb://fixture:{DB}@mongo.example.com:27017/app"),
        }),
        def("MySQL", "mysql", STAGING, 300).columns(row! {
            "database_connection_string" => format!("mysql://fixture:{DB}@mysql.example.com:3306/app"),
            "database_query" => "SELECT 1",
        }),
        def("Oracle", "oracledb", STAGING, 300)
            .retries(2, 60)
            .columns(row! {
                "database_connection_string" => "oracle.example.com:1521/ORCLPDB1", "basic_auth_user" => "fixture",
                "basic_auth_pass" => DB, "database_query" => "SELECT 1 FROM DUAL",
            })
            .plan(Plan::checked(
                6,
                Outages::Since { ms_before_now: oracle_down_ms, message: "ORA-12541: Cannot connect. No listener at host oracle.example.com port 1521." },
            )),
        def("PostgreSQL", "postgres", STAGING, 300).columns(row! {
            "database_connection_string" => format!("postgres://fixture:{DB}@pg.example.com:5432/app"),
            "database_query" => "SELECT 1",
        }),
        def("RADIUS", "radius", EDGE, 300).columns(row! {
            "hostname" => "radius.example.com", "port" => 1812, "radius_username" => "fixture",
            "radius_password" => "fixture-radius-password", "radius_secret" => "fixture-radius-secret",
            "radius_called_station_id" => "00-00-5E-00-53-00", "radius_calling_station_id" => "00-00-5E-00-53-01",
        }),
        def("Redis", "redis", STAGING, 60).columns(row! {
            "database_connection_string" => "redis://:fixture-redis-password@redis.example.com:6379",
        }),
        def("Game server", "gamedig", None, 300).columns(row! {
            "game" => "minecraft", "hostname" => "mc.example.com", "port" => 25565, "gamedig_given_port_only" => true,
        }),
        def("Steam server", "steam", None, 300).columns(row! { "hostname" => "steam.example.com", "port" => 27015 }),
    ]
}

fn monitors(conn: &Connection, start: DateTime<Utc>) -> Result<Vec<Monitor>> {
    let mut ids: HashMap<&str, i64> = HashMap::new();
    let mut monitors = Vec::new();
    for (i, def) in monitor_defs().into_iter().enumerate() {
        let mut row = row! {
            "name" => def.name,
            "active" => def.active,
            "user_id" => 1,
            "interval" => def.interval,
            "type" => def.kind,
            "weight" => 2000,
            "created_date" => ts(start + Duration::minutes(i as i64)),
            "maxretries" => def.maxretries,
            "retry_interval" => def.retry_interval,
            "resend_interval" => def.resend_interval,
            "upside_down" => def.upside_down,
            "timeout" => def.interval as f64 * 0.8,
            "parent" => def.parent.map(|name| ids[name]),
        };
        row.extend(def.columns);
        let id = put(conn, "monitor", row)?;
        ids.insert(def.name, id);
        monitors.push(Monitor {
            id,
            name: def.name,
            kind: def.kind,
            interval: def.interval,
            retry_interval: def.retry_interval,
            maxretries: def.maxretries,
            resend_interval: def.resend_interval,
            upside_down: def.upside_down,
            plan: def.plan,
        });
    }
    Ok(monitors)
}

fn tls_and_expiry(conn: &Connection, id: &impl Fn(&str) -> i64) -> Result<()> {
    for (monitor, host, days) in [
        ("API", "api.example.com", 12),
        ("Homepage", "www.example.com", 74),
    ] {
        let valid_to = now() + Duration::days(days);
        let info = json!({
            "valid": true,
            "certInfo": {
                "subject": { "CN": host },
                "issuer": { "C": "US", "O": "Fixture CA", "CN": "Fixture Intermediate R1" },
                "subjectaltname": format!("DNS:{host}"),
                "valid_from": (valid_to - Duration::days(90)).format("%b %e %H:%M:%S %Y GMT").to_string(),
                "valid_to": valid_to.format("%b %e %H:%M:%S %Y GMT").to_string(),
                "serialNumber": "0F1E2D3C4B5A69788796A5B4C3D2E1F0",
                "fingerprint": "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD",
                "validTo": valid_to.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
                "validFor": [host],
                "daysRemaining": days,
                "certType": "server",
                "issuerCertificate": {
                    "subject": { "C": "US", "O": "Fixture CA", "CN": "Fixture Intermediate R1" },
                    "fingerprint": "11:22:33:44:55:66:77:88:99:00:AA:BB:CC:DD:EE:FF:11:22:33:44",
                    "daysRemaining": 900,
                    "certType": "intermediate CA",
                },
            },
            "hostnameMatchMonitorUrl": true,
        });
        put(
            conn,
            "monitor_tls_info",
            row! { "monitor_id" => id(monitor), "info_json" => info.to_string() },
        )?;
    }
    // API's certificate has 12 days left, so the 21 and 14 day reminders went out.
    for days in [21, 14] {
        put(
            conn,
            "notification_sent_history",
            row! { "type" => "certificate", "monitor_id" => id("API"), "days" => days },
        )?;
    }

    let millis = |time: DateTime<Utc>| time.format("%Y-%m-%d %H:%M:%S%.3f").to_string();
    for (domain, expiry_days, notified) in
        [("example.com", 210, None), ("example.org", 10, Some(14))]
    {
        put(
            conn,
            "domain_expiry",
            row! {
                "last_check" => millis(now() - Duration::hours(3)),
                "domain" => domain,
                "expiry" => millis(now() + Duration::days(expiry_days)),
                "last_expiry_notification_sent" => notified,
            },
        )?;
    }
    Ok(())
}

/// API: the single window and the weekly Saturday window below.
fn api_in_maintenance(time_ms: i64) -> bool {
    let single_start = Utc
        .with_ymd_and_hms(2026, 9, 20, 22, 0, 0)
        .single()
        .expect("valid time");
    let single = (single_start.timestamp_millis()
        ..(single_start + Duration::hours(4)).timestamp_millis())
        .contains(&time_ms);
    let time = utc(time_ms);
    single || (time.weekday() == Weekday::Sat && time.hour() == 2)
}

/// Shop keyword: days 1, 15 and the last day of each month, 01:00 to 01:30 UTC.
fn shop_in_maintenance(time_ms: i64) -> bool {
    let time = utc(time_ms);
    let last_day = (time + Duration::days(1)).day() == 1;
    (time.day() == 1 || time.day() == 15 || last_day) && time.hour() == 1 && time.minute() < 30
}

/// Gateway ping: cron `30 3 * * 1-5` in Asia/Kolkata for 15 minutes.
fn gateway_in_maintenance(time_ms: i64) -> bool {
    let ist = FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("valid offset");
    let local = utc(time_ms).with_timezone(&ist);
    let start = NaiveTime::from_hms_opt(3, 30, 0).expect("valid time");
    let weekday = local.weekday().number_from_monday() <= 5;
    weekday && local.time() >= start && local.time() < start + Duration::minutes(15)
}

fn utc(time_ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(time_ms).expect("fixture times are in range")
}

fn maintenance(conn: &Connection, start: DateTime<Utc>, id: &impl Fn(&str) -> i64) -> Result<()> {
    let date = |time: DateTime<Utc>| ts(time);
    let windows: [(Row, Vec<&str>); 7] = [
        (
            row! {
                "title" => "Emergency switch", "description" => "Manual window, switched off again.",
                "strategy" => "manual", "active" => false, "timezone" => "SAME_AS_SERVER",
            },
            vec!["Docker app"],
        ),
        (
            row! {
                "title" => "Database migration", "description" => "One-off window for the **API** database move.",
                "strategy" => "single", "start_date" => "2026-09-20 22:00:00", "end_date" => "2026-09-21 02:00:00",
                "timezone" => "UTC", "duration" => 0,
            },
            vec!["API"],
        ),
        (
            row! {
                "title" => "Weekly patching", "description" => "Saturday patch window.",
                "strategy" => "recurring-weekday", "start_date" => date(start), "start_time" => "02:00", "end_time" => "03:00",
                "weekdays" => "[6]", "days_of_month" => "[]", "cron" => "0 2 * * 6", "duration" => 3600,
                "timezone" => "UTC", "last_start_date" => "2026-09-26 02:00:00",
            },
            vec!["API"],
        ),
        (
            row! {
                "title" => "Staging reset", "description" => "Staging is rebuilt every three days.",
                "strategy" => "recurring-interval", "interval_day" => 3, "start_date" => date(start),
                "start_time" => "04:00", "end_time" => "04:30", "weekdays" => "[]", "days_of_month" => "[]",
                "cron" => "0 4  * * *", "duration" => 1800, "timezone" => "UTC", "last_start_date" => "2026-09-30 04:00:00",
            },
            vec!["Staging"],
        ),
        (
            row! {
                "title" => "Shop reindex", "description" => "Search reindex on the 1st, 15th and last day.",
                "strategy" => "recurring-day-of-month", "start_date" => date(start), "start_time" => "01:00",
                "end_time" => "01:30", "weekdays" => "[]", "days_of_month" => "[1,15,\"lastDay1\"]",
                "cron" => "0 1 1,15,L * *", "duration" => 1800, "timezone" => "UTC", "last_start_date" => "2026-09-30 01:00:00",
            },
            vec!["Shop keyword"],
        ),
        (
            row! {
                "title" => "Carrier window", "description" => "Upstream carrier maintenance, weekdays.",
                "strategy" => "cron", "cron" => "30 3 * * 1-5", "duration" => 900, "start_date" => date(start),
                "weekdays" => "[]", "days_of_month" => "[]", "timezone" => "Asia/Kolkata",
                "last_start_date" => "2026-09-30 22:00:00",
            },
            vec!["Gateway ping"],
        ),
        (
            row! {
                "title" => "Planned datacenter move", "description" => "Announced, not started yet.",
                "strategy" => "single", "start_date" => "2026-10-24 20:00:00", "end_date" => "2026-10-25 04:00:00",
                "timezone" => "SAME_AS_SERVER", "duration" => 0,
            },
            vec!["Production"],
        ),
    ];
    for (i, (mut row, monitors)) in windows.into_iter().enumerate() {
        if !row.iter().any(|(name, _)| *name == "active") {
            row.push(("active", Value::Integer(1)));
        }
        row.push(("user_id", Value::Integer(1)));
        let maintenance_id = put(conn, "maintenance", row)?;
        for monitor in monitors {
            put(
                conn,
                "monitor_maintenance",
                row! { "monitor_id" => id(monitor), "maintenance_id" => maintenance_id },
            )?;
        }
        // Single, weekly and the planned move are announced on the public page.
        if matches!(i, 1 | 2 | 6) {
            put(
                conn,
                "maintenance_status_page",
                row! { "status_page_id" => 1, "maintenance_id" => maintenance_id },
            )?;
        }
    }
    Ok(())
}

fn status_pages(conn: &Connection, start: DateTime<Utc>, id: &impl Fn(&str) -> i64) -> Result<()> {
    let created = ts(start + Duration::days(3));
    let modified = ts(now() - Duration::days(4));
    put(
        conn,
        "status_page",
        row! {
            "slug" => "status", "title" => "Example Status", "description" => "Live status of Example services.",
            "icon" => "/upload/logo1.png", "theme" => "auto", "published" => true, "search_engine_index" => true,
            "show_tags" => true, "password" => Value::Null, "created_date" => created.clone(), "modified_date" => modified.clone(),
            "footer_text" => "Questions? [Contact us](https://www.example.com/contact)",
            "custom_css" => ".title { color: #0b6e4f; }\nfooter { font-size: 0.9em; }",
            "show_powered_by" => true, "analytics_id" => "status.example.com", "show_certificate_expiry" => true,
            "auto_refresh_interval" => 300, "analytics_script_url" => "https://plausible.example.com/js/script.js",
            "analytics_type" => "plausible", "show_only_last_heartbeat" => false, "rss_title" => "Example status feed",
        },
    )?;
    put(
        conn,
        "status_page",
        row! {
            "slug" => "client-acme", "title" => "Acme services", "description" => "Status for Acme Corp.",
            "icon" => "/icon.svg", "theme" => "dark", "published" => true, "search_engine_index" => false,
            "show_tags" => false, "password" => STATUS_PAGE_PASSWORD, "created_date" => created.clone(),
            "modified_date" => modified.clone(), "custom_css" => "body { --accent: #ff6600; }", "show_powered_by" => false,
            "show_certificate_expiry" => false, "auto_refresh_interval" => 60,
        },
    )?;
    put(
        conn,
        "status_page",
        row! {
            "slug" => "internal", "title" => "Internal", "icon" => "/icon.svg", "theme" => "light", "published" => false,
            "search_engine_index" => false, "show_tags" => true, "created_date" => created, "modified_date" => modified,
            "show_powered_by" => true, "analytics_id" => "fixture-umami-id", "analytics_type" => "umami",
            "analytics_script_url" => "https://umami.example.org/script.js", "show_only_last_heartbeat" => true,
        },
    )?;

    for (page, domain) in [
        (1, "status.example.com"),
        (1, "status.example.org"),
        (2, "acme.status.example.net"),
    ] {
        put(
            conn,
            "status_page_cname",
            row! { "status_page_id" => page, "domain" => domain },
        )?;
    }

    // (status page, group name, [(monitor, send_url, custom_url)])
    let groups: [(i64, &str, Vec<PageMember>); 5] = [
        (
            1,
            "Example services",
            vec![
                ("Homepage", true, None),
                ("API", true, Some("https://api.example.com/docs")),
                ("Shop keyword", false, None),
            ],
        ),
        (
            1,
            "Network",
            vec![
                ("Gateway ping", false, None),
                ("DNS A record", false, None),
                ("Nightly backup", false, None),
            ],
        ),
        (
            2,
            "Acme services",
            vec![("Production", false, None), ("Login (OAuth)", true, None)],
        ),
        (
            3,
            "Staging",
            vec![
                ("Staging", false, None),
                ("Oracle", false, None),
                ("Manual unset", false, None),
            ],
        ),
        (3, "Empty section", vec![]),
    ];
    for (weight, (page, name, members)) in groups.into_iter().enumerate() {
        let group_id = put(
            conn,
            "group",
            row! {
                "name" => name, "created_date" => ts(start + Duration::days(3)), "public" => true, "active" => true,
                "weight" => weight as i64 + 1, "status_page_id" => page,
            },
        )?;
        for (position, (monitor, send_url, custom_url)) in members.into_iter().enumerate() {
            put(
                conn,
                "monitor_group",
                row! {
                    "monitor_id" => id(monitor), "group_id" => group_id, "weight" => position as i64 + 1,
                    "send_url" => send_url, "custom_url" => custom_url,
                },
            )?;
        }
    }

    put(
        conn,
        "incident",
        row! {
            "title" => "Slow checkout", "content" => "We are investigating slow responses in the **shop**.",
            "style" => "warning", "created_date" => ts(now() - Duration::hours(5)),
            "last_updated_date" => ts(now() - Duration::hours(1)), "pin" => true, "active" => true, "status_page_id" => 1,
        },
    )?;
    put(
        conn,
        "incident",
        row! {
            "title" => "Login outage", "content" => "Login was unavailable for 20 minutes. Resolved.",
            "style" => "danger", "created_date" => ts(now() - Duration::days(40)),
            "last_updated_date" => ts(now() - Duration::days(40)), "pin" => false, "active" => true, "status_page_id" => 2,
        },
    )?;
    Ok(())
}
