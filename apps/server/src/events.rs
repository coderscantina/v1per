//! Topic event logs and the hub that wakes subscriptions. A topic's head is its revision;
//! snapshots and commands read it inside their own transaction.

use std::sync::Arc;

use serde::Deserialize;
use sqlx::SqliteConnection;
use tokio::sync::{Semaphore, broadcast, watch};
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{
    ChannelId, ComponentId, DeliveryId, IncidentId, MaintenanceId, MonitorId, PageId, ProbeId,
};
use v1per_protocol::live::LiveEvent;

/// Topic names a lagging subscriber may fall behind by before it re-reads from its cursor.
const WAKE_CAPACITY: usize = 1024;
/// Catch-up pages read and serialized at once across the instance.
const REPLAY_PERMITS: usize = 2;

pub async fn head(conn: &mut SqliteConnection, topic: &Topic) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COALESCE(MAX(seq), 0) FROM topic_events WHERE topic = ?")
        .bind(topic.as_str())
        .fetch_one(conn)
        .await
}

/// Appends an event and returns its sequence. Callers hold the write transaction.
pub async fn record(
    conn: &mut SqliteConnection,
    topic: &Topic,
    kind: &str,
    payload: &serde_json::Value,
    now: i64,
) -> Result<i64, sqlx::Error> {
    let seq = head(&mut *conn, topic).await? + 1;
    sqlx::query(
        "INSERT INTO topic_events (topic, seq, kind, payload, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(topic.as_str())
    .bind(seq)
    .bind(kind)
    .bind(payload.to_string())
    .bind(now)
    .execute(conn)
    .await?;
    Ok(seq)
}

/// What an event is about, from the ID keys of its payload.
#[derive(Default, Deserialize)]
struct Subjects {
    monitor: Option<MonitorId>,
    probe: Option<ProbeId>,
    channel: Option<ChannelId>,
    delivery: Option<DeliveryId>,
    incident: Option<IncidentId>,
    maintenance: Option<MaintenanceId>,
    component: Option<ComponentId>,
    page: Option<PageId>,
}

/// Up to `limit` events of `topic` after `after`, in sequence order.
pub async fn after(
    conn: &mut SqliteConnection,
    topic: &Topic,
    after: i64,
    limit: usize,
) -> Result<Vec<LiveEvent>, sqlx::Error> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT seq, kind, payload FROM topic_events
         WHERE topic = ? AND seq > ? ORDER BY seq LIMIT ?",
    )
    .bind(topic.as_str())
    .bind(after)
    .bind(limit as i64)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(seq, kind, payload)| {
            let subjects: Subjects = serde_json::from_str(&payload).unwrap_or_default();
            LiveEvent {
                seq,
                kind,
                monitor: subjects.monitor,
                probe: subjects.probe,
                channel: subjects.channel,
                delivery: subjects.delivery,
                incident: subjects.incident,
                maintenance: subjects.maintenance,
                component: subjects.component,
                page: subjects.page,
            }
        })
        .collect())
}

/// Wakes subscriptions after a command commits. It carries topic names only: a subscriber
/// re-reads the log from its own cursor, so a lagged or missed wake loses nothing.
pub struct Hub {
    wake: broadcast::Sender<Arc<str>>,
    closing: watch::Sender<bool>,
    /// Bounds concurrent catch-up reads, so reconnecting browsers cannot crowd out commands.
    pub replay: Semaphore,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            wake: broadcast::Sender::new(WAKE_CAPACITY),
            closing: watch::Sender::new(false),
            replay: Semaphore::new(REPLAY_PERMITS),
        }
    }
}

impl Hub {
    pub fn publish(&self, topic: &Topic) {
        // No receivers is fine: nobody is subscribed.
        let _ = self.wake.send(topic.as_str().into());
    }

    pub fn subscribe(&self) -> (broadcast::Receiver<Arc<str>>, watch::Receiver<bool>) {
        (self.wake.subscribe(), self.closing.subscribe())
    }

    /// Ends every open subscription, so a stopping server does not wait for browsers.
    pub fn close(&self) {
        self.closing.send_replace(true);
    }
}
