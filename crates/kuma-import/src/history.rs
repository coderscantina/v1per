//! Streaming readers for Kuma heartbeats and `stat_*` rollups, in the shape
//! `v1per-storage` takes them. Mirrored rather than imported so this crate does not build
//! DuckDB; the server copies field by field and adds its own monitor IDs.

use std::collections::{BTreeMap, VecDeque};

use chrono::{DateTime, NaiveDateTime};
use rusqlite::Connection;
use rusqlite::types::Value;

use crate::plan::MonitorId;

/// Kuma's heartbeat statuses with Kuma's values, as `v1per_storage::Status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Status {
    Down = 0,
    Up = 1,
    Pending = 2,
    Maintenance = 3,
}

/// One Kuma heartbeat, as `v1per_storage::Heartbeat` without probe, HTTP status and
/// certificate expiry, which Kuma does not record. Kuma's `important`, `duration`,
/// `down_count`, `end_time` and `response` columns have no place in storage and are not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KumaHeartbeat {
    /// Kuma's row ID, the resume position.
    pub id: i64,
    pub monitor: MonitorId,
    /// UTC, Unix milliseconds.
    pub time_ms: i64,
    pub status: Status,
    /// Kuma's ping rounded to whole milliseconds.
    pub latency_ms: Option<u32>,
    pub message: String,
    pub retries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeartbeatChunk {
    /// In ascending ID order.
    pub heartbeats: Vec<KumaHeartbeat>,
    /// Pass as `after` to resume behind this chunk.
    pub last_id: i64,
}

/// One `stat_*` row, field for field `v1per_storage::KumaStat`.
#[derive(Debug, Clone, PartialEq)]
pub struct KumaStat {
    /// Bucket start, UTC Unix milliseconds (Kuma stores seconds).
    pub bucket_ms: i64,
    pub ping: Option<f64>,
    pub ping_min: Option<f64>,
    pub ping_max: Option<f64>,
    pub up: u32,
    pub down: u32,
    pub extras: Option<String>,
}

/// Kuma's rollup tables, as `v1per_storage::Resolution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatTable {
    Minutely,
    Hourly,
    Daily,
}

impl StatTable {
    pub const ALL: [StatTable; 3] = [Self::Minutely, Self::Hourly, Self::Daily];

    pub fn name(self) -> &'static str {
        match self {
            Self::Minutely => "stat_minutely",
            Self::Hourly => "stat_hourly",
            Self::Daily => "stat_daily",
        }
    }
}

/// Rows of one monitor and table, ready for `Storage::import_kuma_stats`. A monitor's rows
/// may span several chunks; their bucket ranges never overlap.
#[derive(Debug, Clone, PartialEq)]
pub struct StatChunk {
    pub table: StatTable,
    pub monitor: MonitorId,
    /// In ascending bucket order.
    pub rows: Vec<KumaStat>,
    /// Pass as `after` to resume behind this chunk.
    pub cursor: StatCursor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatCursor {
    pub monitor: MonitorId,
    /// Kuma's `timestamp`: Unix seconds.
    pub timestamp: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryError {
    #[error("{table} row {id}: {reason}")]
    InvalidRow {
        table: &'static str,
        id: i64,
        reason: String,
    },
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Heartbeats at or after a cutoff, chunked by primary key.
pub struct Heartbeats<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) since: String,
    pub(crate) after: i64,
    pub(crate) chunk_size: usize,
    pub(crate) done: bool,
}

impl Iterator for Heartbeats<'_> {
    type Item = Result<HeartbeatChunk, HistoryError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let chunk = self.fetch();
        match &chunk {
            Ok(chunk) => {
                self.done = chunk.heartbeats.len() < self.chunk_size;
                self.after = chunk.last_id;
                if chunk.heartbeats.is_empty() {
                    return None;
                }
            }
            Err(_) => self.done = true,
        }
        Some(chunk)
    }
}

impl Heartbeats<'_> {
    fn fetch(&self) -> Result<HeartbeatChunk, HistoryError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, monitor_id, time, status, ping, msg, retries FROM heartbeat
             WHERE id > ?1 AND time >= ?2 ORDER BY id LIMIT ?3",
        )?;
        let mut rows = stmt.query(rusqlite::params![
            self.after,
            self.since,
            self.chunk_size as i64
        ])?;
        let mut heartbeats = Vec::with_capacity(self.chunk_size);
        while let Some(row) = rows.next()? {
            heartbeats.push(heartbeat(row)?);
        }
        Ok(HeartbeatChunk {
            last_id: heartbeats.last().map_or(self.after, |beat| beat.id),
            heartbeats,
        })
    }
}

/// A row of `id, monitor_id, time, status, ping, msg, retries`.
fn heartbeat(row: &rusqlite::Row<'_>) -> Result<KumaHeartbeat, HistoryError> {
    let id: i64 = row.get(0)?;
    let invalid = |reason: &str| HistoryError::InvalidRow {
        table: "heartbeat",
        id,
        reason: reason.to_owned(),
    };
    let time: String = row.get(2)?;
    Ok(KumaHeartbeat {
        id,
        monitor: MonitorId(row.get(1)?),
        time_ms: parse_time(&time).ok_or_else(|| invalid("time is not a date"))?,
        status: match row.get::<_, i64>(3)? {
            0 => Status::Down,
            1 => Status::Up,
            2 => Status::Pending,
            3 => Status::Maintenance,
            _ => return Err(invalid("status is not 0 to 3")),
        },
        latency_ms: match row.get::<_, Value>(4)? {
            Value::Integer(ms) => u32::try_from(ms).ok(),
            Value::Real(ms) if ms >= 0.0 => Some(ms.round().min(f64::from(u32::MAX)) as u32),
            _ => None,
        },
        message: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        retries: u32::try_from(row.get::<_, i64>(6)?).unwrap_or_default(),
    })
}

/// A monitor's newest heartbeat and when its status last changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LatestHeartbeat {
    pub heartbeat: KumaHeartbeat,
    /// The newest important beat, UTC Unix milliseconds; the newest beat without one.
    pub changed_ms: i64,
}

/// The newest heartbeat of every monitor that has one, by Kuma's `(monitor_id, time)` index.
pub(crate) fn latest(
    conn: &Connection,
) -> Result<BTreeMap<MonitorId, LatestHeartbeat>, HistoryError> {
    let mut stmt = conn.prepare(
        "SELECT h.id, h.monitor_id, h.time, h.status, h.ping, h.msg, h.retries,
                (SELECT time FROM heartbeat WHERE monitor_id = m.id AND important = 1
                 ORDER BY time DESC LIMIT 1)
         FROM monitor m
         JOIN heartbeat h ON h.id = (SELECT id FROM heartbeat WHERE monitor_id = m.id
                                     ORDER BY time DESC, id DESC LIMIT 1)",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let heartbeat = heartbeat(row)?;
        let changed_ms = row
            .get::<_, Option<String>>(7)?
            .as_deref()
            .and_then(parse_time)
            .unwrap_or(heartbeat.time_ms);
        out.insert(
            heartbeat.monitor,
            LatestHeartbeat {
                heartbeat,
                changed_ms,
            },
        );
    }
    Ok(out)
}

/// One `stat_*` table in `(monitor, timestamp)` order, one monitor per chunk.
pub struct Stats<'a> {
    pub(crate) conn: &'a Connection,
    pub(crate) table: StatTable,
    pub(crate) fetched_until: (i64, i64),
    pub(crate) chunk_size: usize,
    pub(crate) buffer: VecDeque<(i64, i64, KumaStat)>,
    pub(crate) exhausted: bool,
}

impl Iterator for Stats<'_> {
    type Item = Result<StatChunk, HistoryError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buffer.is_empty()
            && !self.exhausted
            && let Err(error) = self.fill()
        {
            self.exhausted = true;
            return Some(Err(error));
        }
        let monitor = self.buffer.front()?.0;
        let mut rows = Vec::new();
        let mut timestamp = 0;
        while let Some((_, ts, stat)) = self.buffer.pop_front_if(|(m, _, _)| *m == monitor) {
            timestamp = ts;
            rows.push(stat);
        }
        Some(Ok(StatChunk {
            table: self.table,
            monitor: MonitorId(monitor),
            rows,
            cursor: StatCursor {
                monitor: MonitorId(monitor),
                timestamp,
            },
        }))
    }
}

impl Stats<'_> {
    fn fill(&mut self) -> Result<(), HistoryError> {
        let table = self.table.name();
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT id, monitor_id, timestamp, ping, ping_min, ping_max, up, down, extras FROM {table}
             WHERE monitor_id > ?1 OR (monitor_id = ?1 AND timestamp > ?2)
             ORDER BY monitor_id, timestamp LIMIT ?3"
        ))?;
        let (monitor, timestamp) = self.fetched_until;
        let mut rows = stmt.query(rusqlite::params![
            monitor,
            timestamp,
            self.chunk_size as i64
        ])?;
        let mut fetched = 0;
        while let Some(row) = rows.next()? {
            fetched += 1;
            let id: i64 = row.get(0)?;
            let count = |index| -> Result<u32, HistoryError> {
                u32::try_from(row.get::<_, i64>(index)?).map_err(|_| HistoryError::InvalidRow {
                    table,
                    id,
                    reason: "up or down is negative".to_owned(),
                })
            };
            let (monitor, timestamp): (i64, i64) = (row.get(1)?, row.get(2)?);
            self.buffer.push_back((
                monitor,
                timestamp,
                KumaStat {
                    bucket_ms: timestamp.saturating_mul(1000),
                    ping: row.get(3)?,
                    ping_min: row.get(4)?,
                    ping_max: row.get(5)?,
                    up: count(6)?,
                    down: count(7)?,
                    extras: row.get(8)?,
                },
            ));
            self.fetched_until = (monitor, timestamp);
        }
        self.exhausted = fetched < self.chunk_size;
        Ok(())
    }
}

/// Rows per monitor of one `stat_*` table.
pub(crate) fn stat_rows(
    conn: &Connection,
    table: StatTable,
) -> rusqlite::Result<BTreeMap<MonitorId, u64>> {
    let sql = format!(
        "SELECT monitor_id, count(*) FROM {} GROUP BY monitor_id",
        table.name()
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map([], |row| {
        Ok((MonitorId(row.get(0)?), row.get::<_, i64>(1)? as u64))
    })?
    .collect()
}

/// Heartbeats per monitor at or after the cutoff and before it.
pub(crate) fn heartbeat_rows(
    conn: &Connection,
    since_ms: i64,
) -> rusqlite::Result<BTreeMap<MonitorId, (u64, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT monitor_id, count(*) FILTER (WHERE time >= ?1), count(*) FILTER (WHERE time < ?1)
         FROM heartbeat GROUP BY monitor_id",
    )?;
    stmt.query_map([time_text(since_ms)], |row| {
        Ok((
            MonitorId(row.get(0)?),
            (row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64),
        ))
    })?
    .collect()
}

/// Kuma's up and down sums per monitor over the buckets of `table` in `[from_ms, to_ms)`.
pub(crate) fn uptime_counts(
    conn: &Connection,
    table: StatTable,
    from_ms: i64,
    to_ms: i64,
) -> rusqlite::Result<BTreeMap<MonitorId, (u64, u64)>> {
    let sql = format!(
        "SELECT monitor_id, sum(up), sum(down) FROM {} WHERE timestamp >= ?1 AND timestamp < ?2
         GROUP BY monitor_id",
        table.name()
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map([from_ms.div_euclid(1000), to_ms.div_euclid(1000)], |row| {
        Ok((
            MonitorId(row.get(0)?),
            (row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64),
        ))
    })?
    .collect()
}

/// Start of the newest bucket in any `stat_*` table, UTC Unix milliseconds.
pub(crate) fn newest_bucket_ms(conn: &Connection) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT max(t) FROM (SELECT max(timestamp) AS t FROM stat_minutely
                             UNION ALL SELECT max(timestamp) FROM stat_hourly
                             UNION ALL SELECT max(timestamp) FROM stat_daily)",
        [],
        |row| row.get::<_, Option<i64>>(0),
    )
    .map(|seconds| seconds.map(|s| s.saturating_mul(1000)))
}

/// Heartbeats before and at-or-after the cutoff.
pub(crate) fn count_heartbeats(conn: &Connection, since_ms: i64) -> rusqlite::Result<(u64, u64)> {
    conn.query_row(
        "SELECT count(*) FILTER (WHERE time >= ?1), count(*) FILTER (WHERE time < ?1) FROM heartbeat",
        [time_text(since_ms)],
        |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
    )
}

/// Kuma's heartbeat time format, which sorts like the time it names.
pub(crate) fn time_text(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S%.3f").to_string())
        .unwrap_or_default()
}

/// Kuma's UTC `DATETIME` text, with or without milliseconds, as Unix milliseconds.
pub(crate) fn parse_time(text: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(text.trim(), "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .map(|time| time.and_utc().timestamp_millis())
}
