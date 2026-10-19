//! The agent's bounded result buffer: `buffer.sqlite` in its data directory, a queue of sealed
//! batches delivered oldest first.
//!
//! A small SQLite file rather than an append log: dropping the oldest batch, removing a
//! delivered one and surviving a crash mid-write are single transactions instead of file
//! rewrites and recovery code, and SQLx is already in the binary. A batch never changes once
//! sealed, so its ID (`<buffer id>-<sequence>`) names the same results on every resend. WAL
//! with `synchronous=NORMAL` survives an agent crash; a power loss may drop the last commits.

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use super::wire::ProbeResult;

pub const BUFFER_FILE: &str = "buffer.sqlite";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    id        INTEGER PRIMARY KEY CHECK (id = 1),
    buffer_id TEXT NOT NULL,
    dropped   INTEGER NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS batches (
    seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    results INTEGER NOT NULL,
    bytes   INTEGER NOT NULL,
    body    BLOB NOT NULL
) STRICT;
";

/// A sealed batch waiting for delivery.
pub struct Sealed {
    seq: i64,
    pub id: String,
    pub results: Vec<ProbeResult>,
}

#[derive(Default)]
struct Totals {
    bytes: u64,
    results: u64,
    dropped: u64,
}

pub struct Buffer {
    pool: SqlitePool,
    /// Random per buffer file, so batch IDs of a new buffer never repeat old ones.
    id: String,
    max_bytes: u64,
    totals: Mutex<Totals>,
}

impl Buffer {
    pub async fn open(dir: &Path, max_bytes: u64) -> sqlx::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(dir.join(BUFFER_FILE))
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        sqlx::query("INSERT OR IGNORE INTO meta (id, buffer_id, dropped) VALUES (1, ?, 0)")
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(&pool)
            .await?;
        let (id, dropped): (String, i64) =
            sqlx::query_as("SELECT buffer_id, dropped FROM meta WHERE id = 1")
                .fetch_one(&pool)
                .await?;
        let (bytes, results): (i64, i64) = sqlx::query_as(
            "SELECT coalesce(sum(bytes), 0), coalesce(sum(results), 0) FROM batches",
        )
        .fetch_one(&pool)
        .await?;
        Ok(Self {
            pool,
            id,
            max_bytes,
            totals: Mutex::new(Totals {
                bytes: bytes as u64,
                results: results as u64,
                dropped: dropped as u64,
            }),
        })
    }

    /// Results waiting for delivery.
    pub fn buffered(&self) -> u64 {
        self.totals.lock().expect("buffer totals").results
    }

    /// Results dropped for space since the buffer was created.
    pub fn dropped(&self) -> u64 {
        self.totals.lock().expect("buffer totals").dropped
    }

    /// Seals `results` as one batch. Beyond the size limit the oldest batches are dropped and
    /// counted; the newest batch always stays.
    pub async fn push(&self, results: &[ProbeResult]) -> sqlx::Result<()> {
        let body = serde_json::to_vec(results).expect("results serialize");
        let mut tx = self.pool.begin().await?;
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO batches (results, bytes, body) VALUES (?, ?, ?) RETURNING seq",
        )
        .bind(results.len() as i64)
        .bind(body.len() as i64)
        .bind(&body)
        .fetch_one(&mut *tx)
        .await?;
        let (mut bytes, mut buffered, mut dropped) = {
            let totals = self.totals.lock().expect("buffer totals");
            (
                totals.bytes + body.len() as u64,
                totals.results + results.len() as u64,
                totals.dropped,
            )
        };
        while bytes > self.max_bytes {
            let oldest: Option<(i64, i64)> = sqlx::query_as(
                "DELETE FROM batches WHERE seq = (SELECT min(seq) FROM batches WHERE seq < ?)
                 RETURNING results, bytes",
            )
            .bind(seq)
            .fetch_optional(&mut *tx)
            .await?;
            let Some((results, size)) = oldest else {
                break;
            };
            bytes -= size as u64;
            buffered -= results as u64;
            dropped += results as u64;
        }
        sqlx::query("UPDATE meta SET dropped = ? WHERE id = 1")
            .bind(dropped as i64)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        *self.totals.lock().expect("buffer totals") = Totals {
            bytes,
            results: buffered,
            dropped,
        };
        Ok(())
    }

    /// The oldest batch waiting for delivery.
    pub async fn oldest(&self) -> sqlx::Result<Option<Sealed>> {
        let row: Option<(i64, Vec<u8>)> =
            sqlx::query_as("SELECT seq, body FROM batches ORDER BY seq LIMIT 1")
                .fetch_optional(&self.pool)
                .await?;
        let Some((seq, body)) = row else {
            return Ok(None);
        };
        Ok(Some(Sealed {
            seq,
            id: format!("{}-{seq}", self.id),
            results: serde_json::from_slice(&body)
                .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
        }))
    }

    /// Forgets a delivered batch. One dropped for space meanwhile is already gone.
    pub async fn remove(&self, batch: &Sealed) -> sqlx::Result<()> {
        let removed: Option<(i64, i64)> =
            sqlx::query_as("DELETE FROM batches WHERE seq = ? RETURNING results, bytes")
                .bind(batch.seq)
                .fetch_optional(&self.pool)
                .await?;
        if let Some((results, bytes)) = removed {
            let mut totals = self.totals.lock().expect("buffer totals");
            totals.results -= results as u64;
            totals.bytes -= bytes as u64;
        }
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

#[cfg(test)]
mod tests {
    use v1per_protocol::ids::MonitorId;
    use v1per_protocol::operations::monitors::HeartbeatStatus;

    use super::*;

    fn result(time: i64) -> ProbeResult {
        ProbeResult {
            monitor: MonitorId::new(1),
            revision: 1,
            time,
            status: HeartbeatStatus::Up,
            retries: 0,
            latency_ms: Some(5),
            message: "200 - OK".into(),
            http_status: Some(200),
            cert_expiry: None,
            tls: None,
        }
    }

    /// Batches come out oldest first with stable IDs; a full buffer drops whole old batches,
    /// counts them, and keeps that count across a reopen.
    #[tokio::test]
    async fn drains_in_order_and_drops_the_oldest_when_full() {
        let dir = tempfile::tempdir().unwrap();
        let one = serde_json::to_vec(&[result(0)]).unwrap().len() as u64;
        let buffer = Buffer::open(dir.path(), 3 * one).await.unwrap();
        for time in 0..5 {
            buffer.push(&[result(time)]).await.unwrap();
        }
        assert_eq!((buffer.buffered(), buffer.dropped()), (3, 2));
        let oldest = buffer.oldest().await.unwrap().unwrap();
        assert_eq!(oldest.results, [result(2)]);
        assert_eq!(buffer.oldest().await.unwrap().unwrap().id, oldest.id);
        buffer.remove(&oldest).await.unwrap();
        assert_eq!(buffer.oldest().await.unwrap().unwrap().results, [result(3)]);
        buffer.close().await;

        let reopened = Buffer::open(dir.path(), 3 * one).await.unwrap();
        assert_eq!((reopened.buffered(), reopened.dropped()), (2, 2));
        let next = reopened.oldest().await.unwrap().unwrap();
        assert_eq!(next.results, [result(3)]);
        assert!(next.id.starts_with(&oldest.id[..36]));
    }
}
