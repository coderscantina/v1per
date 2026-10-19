//! The notification outbox (decision 0005). Whatever causes a message writes one row per active
//! channel in its own transaction: the evaluator with the transition, the certificate check,
//! the probe sweep, a test command. The row holds the event as it was then, without secrets.
//!
//! One worker sends them, at most [`CONCURRENCY`] at once and per channel oldest first, so an
//! up never overtakes the down before it. A failure the provider may get over is retried
//! after 10 seconds, doubling up to 15 minutes, for 24 hours; any other failure is final.
//! Rows stay pending until their outcome is written, so a restart sends what was in flight
//! again. A message for a monitor under maintenance is suppressed when its turn comes.
//! Finished rows form the channel's delivery log; retention deletes them after 30 days.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use tokio::task::JoinSet;
use v1per_notify::{self as notify, ChannelConfig, SendContext};
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{ChannelId, DeliveryId, MonitorId, NodeId};
use v1per_protocol::operations::monitors::MonitorSettings;
use v1per_storage::Status;

use super::config;
use crate::instance;
use crate::state::AppState;

/// Messages sent at once.
const CONCURRENCY: usize = 16;
const FIRST_RETRY_MS: i64 = 10_000;
const LONGEST_RETRY_MS: i64 = 15 * 60_000;
/// After this long a retryable failure is final.
const GIVE_UP_AFTER_MS: i64 = 24 * 60 * 60_000;
/// The longest the worker sleeps without a wake, in case one was missed.
const IDLE_MS: i64 = 60_000;

/// An event as the outbox keeps it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(crate) enum Payload {
    MonitorDown(BeatPayload),
    MonitorUp(BeatPayload),
    CertificateExpiry(CertificatePayload),
    ProbeOffline(ProbePayload),
    Test { channel_name: String },
}

/// The monitor fields Kuma's `monitorJSON` carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MonitorPayload {
    pub id: MonitorId,
    pub node: NodeId,
    pub name: String,
    /// Kuma's type name.
    pub kind: String,
    pub url: Option<String>,
    pub hostname: Option<String>,
    pub port: Option<u16>,
    /// Enclosing group monitors, outermost first.
    pub groups: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BeatPayload {
    pub monitor: MonitorPayload,
    pub time: i64,
    pub msg: String,
    pub ping: Option<i64>,
    pub important: bool,
    /// Seconds since the beat before.
    pub duration: i64,
    pub retries: i64,
    /// Up only: the latest important down beat.
    pub last_down_time: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CertKind {
    Server,
    IntermediateCa,
    RootCa,
    SelfSigned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CertificatePayload {
    pub monitor_name: String,
    pub monitor_url: String,
    pub cert_type: CertKind,
    pub subject_cn: String,
    pub days_remaining: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProbePayload {
    pub probe_name: String,
    pub last_seen: i64,
}

fn time(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

impl Payload {
    fn event_name(&self) -> &'static str {
        match self {
            Payload::MonitorDown(_) => "monitor_down",
            Payload::MonitorUp(_) => "monitor_up",
            Payload::CertificateExpiry(_) => "certificate_expiry",
            Payload::ProbeOffline(_) => "probe_offline",
            Payload::Test { .. } => "test",
        }
    }

    /// The event to send, with times in `timezone` and monitor links under `public_url`.
    pub(crate) fn to_event(&self, timezone: Tz, public_url: Option<&str>) -> notify::Event {
        let beat = |beat: &BeatPayload| {
            let monitor = &beat.monitor;
            notify::Beat {
                monitor: notify::Monitor {
                    id: monitor.id.get(),
                    name: monitor.name.clone(),
                    kind: monitor.kind.clone(),
                    subtype: None,
                    url: monitor.url.clone(),
                    hostname: monitor.hostname.clone(),
                    port: monitor.port,
                    groups: monitor.groups.clone(),
                    // v1per tags are plain labels; Kuma's carry a value and colour.
                    tags: (1..)
                        .zip(&monitor.tags)
                        .map(|(tag_id, name)| notify::MonitorTag {
                            tag_id,
                            name: name.clone(),
                            value: String::new(),
                            color: String::new(),
                        })
                        .collect(),
                    link: public_url
                        .map(|base| format!("{base}/n/{}/monitors/{}", monitor.node, monitor.id)),
                },
                heartbeat: notify::Heartbeat {
                    time: time(beat.time),
                    msg: beat.msg.clone(),
                    ping: beat.ping,
                    important: beat.important,
                    duration: beat.duration,
                    retries: beat.retries,
                    response: None,
                    last_down_time: beat.last_down_time.map(time),
                },
                timezone,
            }
        };
        match self {
            Payload::MonitorDown(down) => notify::Event::MonitorDown(beat(down)),
            Payload::MonitorUp(up) => notify::Event::MonitorUp(beat(up)),
            Payload::CertificateExpiry(cert) => {
                notify::Event::CertificateExpiry(notify::CertificateExpiry {
                    monitor_name: cert.monitor_name.clone(),
                    monitor_url: cert.monitor_url.clone(),
                    cert_type: match cert.cert_type {
                        CertKind::Server => notify::CertType::Server,
                        CertKind::IntermediateCa => notify::CertType::IntermediateCa,
                        CertKind::RootCa => notify::CertType::RootCa,
                        CertKind::SelfSigned => notify::CertType::SelfSigned,
                    },
                    subject_cn: cert.subject_cn.clone(),
                    days_remaining: cert.days_remaining,
                })
            }
            Payload::ProbeOffline(probe) => notify::Event::ProbeOffline(notify::ProbeOffline {
                probe_name: probe.probe_name.clone(),
                last_seen: time(probe.last_seen),
                timezone,
            }),
            Payload::Test { channel_name } => notify::Event::Test {
                channel_name: channel_name.clone(),
            },
        }
    }
}

/// A channel a message goes to.
#[derive(sqlx::FromRow)]
struct Recipient {
    id: i64,
    node_id: i64,
}

/// Writes one pending row per recipient and records `channel.delivery` on each channel's
/// node. Returns the topics to wake.
async fn queue(
    conn: &mut SqliteConnection,
    recipients: &[Recipient],
    monitor: Option<MonitorId>,
    transition: Option<i64>,
    payload: &Payload,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    if recipients.is_empty() {
        return Ok(Vec::new());
    }
    let timezone = instance::timezone(conn).await?;
    let summary = payload.to_event(timezone, None).message();
    let json = serde_json::to_string(payload).expect("payloads serialize");
    let mut topics = Vec::new();
    for recipient in recipients {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO notification_outbox (node_id, channel_id, event, monitor_id,
                 transition_id, payload, summary, state, next_attempt_at, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?) RETURNING id",
        )
        .bind(recipient.node_id)
        .bind(recipient.id)
        .bind(payload.event_name())
        .bind(monitor.map(MonitorId::get))
        .bind(transition)
        .bind(&json)
        .bind(&summary)
        .bind(now)
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;
        let topic = Topic::channels(NodeId::new(recipient.node_id));
        let event =
            json!({ "channel": ChannelId::new(recipient.id), "delivery": DeliveryId::new(id) });
        crate::events::record(conn, &topic, "channel.delivery", &event, now).await?;
        if !topics.contains(&topic) {
            topics.push(topic);
        }
    }
    Ok(topics)
}

/// The active channels `monitor` alerts.
async fn monitor_channels(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> sqlx::Result<Vec<Recipient>> {
    sqlx::query_as(
        "SELECT c.id, c.node_id FROM monitor_channels mc JOIN channels c ON c.id = mc.channel_id
         WHERE mc.monitor_id = ? AND c.active = 1 ORDER BY c.id",
    )
    .bind(monitor.get())
    .fetch_all(conn)
    .await
}

/// A monitor-level beat that Kuma notifies about, as the evaluator journals it.
pub(crate) struct Beat<'a> {
    pub monitor: MonitorId,
    pub transition: i64,
    pub status: Status,
    pub time: i64,
    pub message: &'a str,
    pub latency_ms: Option<u32>,
    pub important: bool,
    pub duration_s: i64,
    pub retries: u32,
}

/// Queues Kuma's down or up message for the monitor's channels.
pub(crate) async fn queue_beat(
    conn: &mut SqliteConnection,
    beat: Beat<'_>,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let recipients = monitor_channels(conn, beat.monitor).await?;
    if recipients.is_empty() {
        return Ok(Vec::new());
    }
    let Some(monitor) = snapshot(conn, beat.monitor).await? else {
        return Ok(Vec::new());
    };
    let up = beat.status == Status::Up;
    let last_down_time = if up {
        sqlx::query_scalar(
            "SELECT time FROM monitor_transitions
             WHERE monitor_id = ? AND status = 0 AND important = 1 AND id < ?
             ORDER BY id DESC LIMIT 1",
        )
        .bind(beat.monitor.get())
        .bind(beat.transition)
        .fetch_optional(&mut *conn)
        .await?
    } else {
        None
    };
    let beat_payload = BeatPayload {
        monitor,
        time: beat.time,
        msg: beat.message.to_owned(),
        ping: beat.latency_ms.map(i64::from),
        important: beat.important,
        duration: beat.duration_s,
        retries: i64::from(beat.retries),
        last_down_time,
    };
    let payload = if up {
        Payload::MonitorUp(beat_payload)
    } else {
        Payload::MonitorDown(beat_payload)
    };
    queue(
        conn,
        &recipients,
        Some(beat.monitor),
        Some(beat.transition),
        &payload,
        now,
    )
    .await
}

/// Queues a certificate expiry message for the monitor's channels; false when it has none.
pub(crate) async fn queue_certificate(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    certificate: CertificatePayload,
    now: i64,
) -> sqlx::Result<(bool, Vec<Topic>)> {
    let recipients = monitor_channels(conn, monitor).await?;
    let payload = Payload::CertificateExpiry(certificate);
    let topics = queue(conn, &recipients, Some(monitor), None, &payload, now).await?;
    Ok((!recipients.is_empty(), topics))
}

/// Queues a probe offline message for the active channels on the probe's node.
pub(crate) async fn queue_probe_offline(
    conn: &mut SqliteConnection,
    node: NodeId,
    probe: ProbePayload,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let recipients: Vec<Recipient> = sqlx::query_as(
        "SELECT id, node_id FROM channels WHERE node_id = ? AND active = 1 ORDER BY id",
    )
    .bind(node.get())
    .fetch_all(&mut *conn)
    .await?;
    queue(
        conn,
        &recipients,
        None,
        None,
        &Payload::ProbeOffline(probe),
        now,
    )
    .await
}

/// Queues a test of `config` (Kuma JSON) on `node`, in the log of `channel` when it is saved.
/// The config is sealed into the row, since the form may differ from the saved channel.
pub(crate) async fn queue_test(
    conn: &mut SqliteConnection,
    key: &crate::identity::secrets::SecretKey,
    node: NodeId,
    channel: Option<ChannelId>,
    config: &config::Kuma,
    channel_name: String,
    now: i64,
) -> sqlx::Result<DeliveryId> {
    let payload = Payload::Test { channel_name };
    let summary = payload.to_event(Tz::UTC, None).message();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO notification_outbox (node_id, channel_id, event, payload, summary, config,
             state, next_attempt_at, created_at)
         VALUES (?, ?, 'test', ?, ?, NULL, 'pending', ?, ?) RETURNING id",
    )
    .bind(node.get())
    .bind(channel.map(ChannelId::get))
    .bind(serde_json::to_string(&payload).expect("payloads serialize"))
    .bind(summary)
    .bind(now)
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    let delivery = DeliveryId::new(id);
    sqlx::query("UPDATE notification_outbox SET config = ? WHERE id = ?")
        .bind(config::seal(key, &test_context(delivery), config))
        .bind(id)
        .execute(conn)
        .await?;
    Ok(delivery)
}

fn test_context(delivery: DeliveryId) -> String {
    format!("delivery:{delivery}:config")
}

/// The monitor as Kuma's `monitorJSON` shows it.
async fn snapshot(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
) -> sqlx::Result<Option<MonitorPayload>> {
    let row: Option<(i64, String, String, String, String)> =
        sqlx::query_as("SELECT node_id, name, type, settings, tags FROM monitors WHERE id = ?")
            .bind(monitor.get())
            .fetch_optional(&mut *conn)
            .await?;
    let Some((node, name, kind, settings, tags)) = row else {
        return Ok(None);
    };
    let groups: Vec<String> = sqlx::query_scalar(
        "WITH RECURSIVE up(id, name, depth) AS (
             SELECT g.id, g.name, 0 FROM monitors m JOIN monitors g ON g.id = m.group_id
             WHERE m.id = ?1
             UNION ALL
             SELECT g.id, g.name, up.depth + 1 FROM up
             JOIN monitors m ON m.id = up.id JOIN monitors g ON g.id = m.group_id
             WHERE up.depth < 32
         )
         SELECT name FROM up ORDER BY depth DESC",
    )
    .bind(monitor.get())
    .fetch_all(&mut *conn)
    .await?;
    let (url, hostname, port) = match serde_json::from_str::<MonitorSettings>(&settings) {
        Ok(
            MonitorSettings::Http(http)
            | MonitorSettings::Keyword(v1per_protocol::operations::monitors::KeywordSettings {
                request: http,
                ..
            })
            | MonitorSettings::JsonQuery(v1per_protocol::operations::monitors::JsonQuerySettings {
                request: http,
                ..
            }),
        ) => (Some(http.url), None, None),
        Ok(MonitorSettings::Port(port)) => (None, Some(port.hostname), Some(port.port)),
        Ok(MonitorSettings::Ping(ping)) => (None, Some(ping.hostname), None),
        Ok(MonitorSettings::Dns(dns)) => (None, Some(dns.hostname), Some(dns.port)),
        _ => (None, None, None),
    };
    Ok(Some(MonitorPayload {
        id: monitor,
        node: NodeId::new(node),
        name,
        kind,
        url,
        hostname,
        port,
        groups,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
    }))
}

/// Sends due messages until `stop`, then waits for those in flight.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let ctx = SendContext::new();
    let (mut wake, _) = state.hub.subscribe();
    let mut running: JoinSet<i64> = JoinSet::new();
    let mut in_flight: HashSet<i64> = HashSet::new();
    while !*stop.borrow() {
        let now = state.clock.now_ms();
        let next = match claim(&state, now, &in_flight).await {
            Ok((due, next)) => {
                for id in due.into_iter().take(CONCURRENCY - running.len()) {
                    in_flight.insert(id);
                    let (state, ctx) = (state.clone(), ctx.clone());
                    running.spawn(async move {
                        if let Err(error) = deliver(&state, &ctx, id).await {
                            tracing::error!(%error, delivery = id, "a delivery failed to run; retrying");
                        }
                        id
                    });
                }
                next
            }
            Err(error) => {
                tracing::error!(%error, "reading the notification outbox failed; retrying");
                None
            }
        };
        let sleep_until = next.unwrap_or(now + IDLE_MS).min(now + IDLE_MS);
        tokio::select! {
            _ = state.outbox.notified() => {}
            woken = wake.recv() => if matches!(woken, Err(RecvError::Closed)) { break },
            Some(done) = running.join_next(), if !running.is_empty() => {
                if let Ok(id) = done {
                    in_flight.remove(&id);
                }
            }
            _ = state.clock.sleep_until(sleep_until) => {}
            _ = stop.changed() => {}
        }
    }
    while running.join_next().await.is_some() {}
}

/// Due rows not in flight, at most one per channel, and when the next pending row falls due.
async fn claim(
    state: &AppState,
    now: i64,
    in_flight: &HashSet<i64>,
) -> sqlx::Result<(Vec<i64>, Option<i64>)> {
    let mut conn = state.db.reader.acquire().await?;
    let due: Vec<i64> = sqlx::query_scalar(
        "SELECT o.id FROM notification_outbox o
         WHERE o.state = 'pending' AND o.next_attempt_at <= ?1
           AND (o.event = 'test' OR NOT EXISTS (
                SELECT 1 FROM notification_outbox p
                WHERE p.channel_id = o.channel_id AND p.state = 'pending'
                  AND p.event != 'test' AND p.id < o.id))
         ORDER BY o.next_attempt_at, o.id LIMIT ?2",
    )
    .bind(now)
    .bind((CONCURRENCY + in_flight.len()) as i64)
    .fetch_all(&mut *conn)
    .await?;
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT MIN(next_attempt_at) FROM notification_outbox
         WHERE state = 'pending' AND next_attempt_at > ?",
    )
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    Ok((
        due.into_iter()
            .filter(|id| !in_flight.contains(id))
            .collect(),
        next,
    ))
}

#[derive(sqlx::FromRow)]
struct Pending {
    node_id: i64,
    channel_id: Option<i64>,
    monitor_id: Option<i64>,
    payload: String,
    config: Option<Vec<u8>>,
    attempts: i64,
    created_at: i64,
}

enum Outcome {
    Delivered(String),
    Retry(String),
    Failed(String),
    Suppressed,
}

/// Sends one row and records the outcome.
async fn deliver(state: &AppState, ctx: &SendContext, id: i64) -> anyhow::Result<()> {
    let mut conn = state.db.reader.acquire().await?;
    let row: Option<Pending> = sqlx::query_as(
        "SELECT node_id, channel_id, monitor_id, payload, config, attempts, created_at
         FROM notification_outbox WHERE id = ? AND state = 'pending'",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else {
        return Ok(());
    };
    let payload: Payload = serde_json::from_str(&row.payload)?;
    let test = matches!(payload, Payload::Test { .. });
    let maintenance = match (row.monitor_id, &payload) {
        (Some(monitor), Payload::MonitorDown(_) | Payload::MonitorUp(_)) => {
            sqlx::query_scalar::<_, i64>("SELECT 1 FROM maintenance_state WHERE monitor_id = ?")
                .bind(monitor)
                .fetch_optional(&mut *conn)
                .await?
                .is_some()
        }
        _ => false,
    };
    let outcome = if maintenance {
        Outcome::Suppressed
    } else {
        let config = match (&row.config, row.channel_id) {
            (Some(sealed), _) => Some(config::open(
                &state.secrets,
                &test_context(DeliveryId::new(id)),
                sealed,
            )?),
            (None, Some(channel)) => {
                let channel_row: Option<(Vec<u8>, bool)> =
                    sqlx::query_as("SELECT config, active FROM channels WHERE id = ?")
                        .bind(channel)
                        .fetch_optional(&mut *conn)
                        .await?;
                match channel_row {
                    Some((sealed, true)) => Some(config::open(
                        &state.secrets,
                        &config::channel_context(ChannelId::new(channel)),
                        &sealed,
                    )?),
                    _ => None,
                }
            }
            (None, None) => None,
        };
        match config {
            None => Outcome::Failed("The channel is inactive.".to_owned()),
            Some(config) => match serde_json::from_value::<ChannelConfig>(Value::Object(config)) {
                Err(error) => Outcome::Failed(format!("invalid channel config: {error}")),
                Ok(mut channel) => {
                    let settings = instance::effective(&mut conn, &state.config).await?;
                    if let ChannelConfig::Smtp(smtp) = &mut channel
                        && smtp.host.is_none()
                        && let Some(server) = instance::smtp(&mut conn, &state.secrets).await?
                    {
                        smtp.host = Some(server.host);
                        smtp.port = Some(server.port);
                        smtp.secure = server.secure;
                        smtp.ignore_tls_error = server.ignore_tls_error;
                        smtp.ignore_starttls = server.ignore_starttls;
                        smtp.username = server.username;
                        smtp.password = server.password.map(notify::Secret::new);
                        smtp.from = smtp.from.take().or(Some(server.from));
                    }
                    drop(conn);
                    let event = payload.to_event(settings.timezone, settings.public_url.as_deref());
                    match notify::send(&channel, &event, ctx).await {
                        Ok(delivered) => Outcome::Delivered(delivered.detail),
                        Err(error)
                            if error.is_retryable()
                                && !test
                                && state.clock.now_ms() - row.created_at < GIVE_UP_AFTER_MS =>
                        {
                            Outcome::Retry(error.to_string())
                        }
                        Err(error) => Outcome::Failed(error.to_string()),
                    }
                }
            },
        }
    };
    finish(state, id, &row, outcome).await
}

/// 10 seconds after the first failure, doubling up to 15 minutes.
fn retry_delay(attempts: i64) -> i64 {
    let doublings = attempts.clamp(1, 20) - 1;
    (FIRST_RETRY_MS << doublings).min(LONGEST_RETRY_MS)
}

async fn finish(state: &AppState, id: i64, row: &Pending, outcome: Outcome) -> anyhow::Result<()> {
    let now = state.clock.now_ms();
    let attempts = row.attempts + i64::from(!matches!(outcome, Outcome::Suppressed));
    let (state_name, next, error, detail) = match &outcome {
        Outcome::Delivered(detail) => ("delivered", None, None, Some(detail.as_str())),
        Outcome::Retry(error) => (
            "pending",
            Some(now + retry_delay(attempts)),
            Some(error.as_str()),
            None,
        ),
        Outcome::Failed(error) => ("failed", None, Some(error.as_str()), None),
        Outcome::Suppressed => ("suppressed", None, None, None),
    };
    if let Some(error) = error {
        tracing::warn!(delivery = id, channel = row.channel_id, %error, retry = next.is_some(), "a notification was not delivered");
    }
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let updated = sqlx::query(
        "UPDATE notification_outbox
         SET state = ?, attempts = ?, next_attempt_at = ?, error = ?, detail = ?,
             finished_at = CASE WHEN ? = 'pending' THEN NULL ELSE ? END
         WHERE id = ? AND state = 'pending'",
    )
    .bind(state_name)
    .bind(attempts)
    .bind(next)
    .bind(error)
    .bind(detail)
    .bind(state_name)
    .bind(now)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated == 0 {
        return Ok(());
    }
    let topic = Topic::channels(NodeId::new(row.node_id));
    let event = json!({
        "channel": row.channel_id.map(ChannelId::new),
        "delivery": DeliveryId::new(id),
    });
    crate::events::record(&mut tx, &topic, "channel.delivery", &event, now).await?;
    tx.commit().await?;
    state.hub.publish(&topic);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_to_fifteen_minutes() {
        let delays: Vec<i64> = (1..=9).map(retry_delay).collect();
        assert_eq!(
            delays,
            [10, 20, 40, 80, 160, 320, 640, 900, 900].map(|s| s * 1000)
        );
    }
}
