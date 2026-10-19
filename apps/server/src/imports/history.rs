//! The `stats` and `heartbeats` steps: Kuma history into heartbeat storage in chunks, each
//! followed by a checkpoint.
//!
//! A `stat_*` chunk replaces the span it covers in `kuma_stats`, so a chunk repeated after a
//! crash counts once whatever its boundaries. Heartbeat chunks enter the durable inbox under
//! the batch key `kuma-import:<run>:<n>` of the pseudo probe `kuma-import`; chunking from the
//! checkpoint is deterministic, so a chunk admitted before a crash but not checkpointed comes
//! back with the same key and content and is stored once.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqlitePool};
use v1per_kuma_import::{Source, StatCursor, StatTable, Status as KumaStatus};
use v1per_protocol::operations::imports::ImportStep;
use v1per_storage::{
    BatchId, Heartbeat, KumaStat, MonitorId, ProbeId, Refusal, Resolution, Status, Storage,
};

use super::state::{self, Advance};

/// How long a chunk waits for transfer to make room in the inbox before it asks again.
const BUSY_WAIT: Duration = Duration::from_millis(200);

/// Where `stats` stands: the table it is in and the last row it committed there.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
struct StatsCursor {
    table: usize,
    monitor: Option<i64>,
    timestamp: Option<i64>,
}

pub fn resolution(table: StatTable) -> Resolution {
    match table {
        StatTable::Minutely => Resolution::Minute,
        StatTable::Hourly => Resolution::Hour,
        StatTable::Daily => Resolution::Day,
    }
}

/// Every `stat_*` row of an imported monitor, as Kuma stored it (decision 0008).
pub async fn stats(
    db: &SqlitePool,
    storage: &Storage,
    source: &Source,
    run: i64,
    monitors: &BTreeMap<i64, i64>,
    chunk_rows: usize,
) -> anyhow::Result<()> {
    let mut conn = db.acquire().await?;
    let checkpoint = state::checkpoint_of(&mut conn, run, ImportStep::Stats).await?;
    if checkpoint.done {
        return Ok(());
    }
    let mut cursor: StatsCursor = match &checkpoint.cursor {
        Some(text) => serde_json::from_str(text)?,
        None => StatsCursor::default(),
    };
    let mut chunks = checkpoint.chunks;
    while let Some(table) = StatTable::ALL.get(cursor.table).copied() {
        let after = cursor
            .monitor
            .zip(cursor.timestamp)
            .map(|(monitor, timestamp)| StatCursor {
                monitor: v1per_kuma_import::MonitorId(monitor),
                timestamp,
            });
        for chunk in source.stats(table, after, chunk_rows) {
            let chunk = chunk?;
            let rows = chunk.rows.len() as i64;
            let imported = monitors.get(&chunk.monitor.0).copied();
            if let Some(monitor) = imported {
                let rows = chunk
                    .rows
                    .into_iter()
                    .map(|row| KumaStat {
                        bucket_ms: row.bucket_ms,
                        ping: row.ping,
                        ping_min: row.ping_min,
                        ping_max: row.ping_max,
                        up: row.up,
                        down: row.down,
                        extras: row.extras,
                    })
                    .collect();
                storage
                    .import_kuma_stats(MonitorId(monitor), resolution(table), rows)
                    .await?;
            }
            chunks += 1;
            cursor.monitor = Some(chunk.cursor.monitor.0);
            cursor.timestamp = Some(chunk.cursor.timestamp);
            let (rows, skipped) = if imported.is_some() {
                (rows, 0)
            } else {
                (0, rows)
            };
            let text = serde_json::to_string(&cursor)?;
            state::advance(
                &mut conn,
                run,
                ImportStep::Stats,
                Advance {
                    cursor: Some(&text),
                    chunks,
                    rows,
                    skipped,
                    done: false,
                },
            )
            .await?;
        }
        cursor = StatsCursor {
            table: cursor.table + 1,
            ..StatsCursor::default()
        };
        let text = serde_json::to_string(&cursor)?;
        state::advance(
            &mut conn,
            run,
            ImportStep::Stats,
            Advance {
                cursor: Some(&text),
                chunks,
                rows: 0,
                skipped: 0,
                done: cursor.table == StatTable::ALL.len(),
            },
        )
        .await?;
    }
    Ok(())
}

/// Raw heartbeats at or after `since_ms` of every imported monitor.
pub async fn heartbeats(
    db: &SqlitePool,
    storage: &Storage,
    source: &Source,
    run: i64,
    monitors: &BTreeMap<i64, i64>,
    since_ms: i64,
    chunk_rows: usize,
) -> anyhow::Result<()> {
    let mut conn = db.acquire().await?;
    let checkpoint = state::checkpoint_of(&mut conn, run, ImportStep::Heartbeats).await?;
    if checkpoint.done {
        return Ok(());
    }
    let mut cursor = checkpoint.cursor;
    let after = cursor.as_deref().map(str::parse::<i64>).transpose()?;
    let mut chunks = checkpoint.chunks;
    for chunk in source.heartbeats(since_ms, after, chunk_rows) {
        let chunk = chunk?;
        let read = chunk.heartbeats.len() as i64;
        let beats: Vec<Heartbeat> = chunk
            .heartbeats
            .into_iter()
            .filter_map(|beat| {
                Some(Heartbeat {
                    monitor: MonitorId(*monitors.get(&beat.monitor.0)?),
                    probe: Some(ProbeId::KUMA_IMPORT),
                    time_ms: beat.time_ms,
                    status: match beat.status {
                        KumaStatus::Down => Status::Down,
                        KumaStatus::Up => Status::Up,
                        KumaStatus::Pending => Status::Pending,
                        KumaStatus::Maintenance => Status::Maintenance,
                    },
                    latency_ms: beat.latency_ms,
                    message: beat.message,
                    retries: beat.retries,
                    http_status: None,
                    cert_expiry_ms: None,
                })
            })
            .collect();
        chunks += 1;
        let imported = beats.len() as i64;
        if !beats.is_empty() {
            let batch = BatchId {
                probe: ProbeId::KUMA_IMPORT,
                id: format!("kuma-import:{run}:{chunks}"),
            };
            admit(storage, batch, &beats).await?;
        }
        let last = cursor.insert(chunk.last_id.to_string());
        state::advance(
            &mut conn,
            run,
            ImportStep::Heartbeats,
            Advance {
                cursor: Some(last),
                chunks,
                rows: imported,
                skipped: read - imported,
                done: false,
            },
        )
        .await?;
    }
    state::advance(
        &mut conn,
        run,
        ImportStep::Heartbeats,
        Advance {
            cursor: cursor.as_deref(),
            chunks,
            rows: 0,
            skipped: 0,
            done: true,
        },
    )
    .await?;
    Ok(())
}

/// Seeds each imported monitor's state, and that of the probes assigned to it, from its newest
/// Kuma heartbeat: a monitor down at cutover stays down without alerting again, a probe goes on
/// counting Kuma's retries, and a push monitor keeps its deadline. Runs in the final
/// transaction, so it happens once.
pub async fn seed_state(
    conn: &mut SqliteConnection,
    source: &Source,
    monitors: &BTreeMap<i64, i64>,
) -> anyhow::Result<()> {
    for (kuma, latest) in source.latest_heartbeats()? {
        let Some(&monitor) = monitors.get(&kuma.0) else {
            continue;
        };
        let beat = &latest.heartbeat;
        let status = beat.status as i64;
        sqlx::query(
            "INSERT INTO monitor_state (monitor_id, status, message, latency_ms, retries,
                                        down_count, beat_at, changed_at)
             VALUES (?, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(monitor)
        .bind(status)
        .bind(&beat.message)
        .bind(beat.latency_ms)
        .bind(beat.retries)
        .bind(beat.time_ms)
        .bind(latest.changed_ms.min(beat.time_ms))
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO probe_state (monitor_id, probe_id, status, message, latency_ms, retries,
                                      checked_at, received_at)
             SELECT monitor_id, probe_id, ?, ?, ?, ?, ?, ? FROM monitor_probes
             WHERE monitor_id = ?",
        )
        .bind(status)
        .bind(&beat.message)
        .bind(beat.latency_ms)
        .bind(beat.retries)
        .bind(beat.time_ms)
        .bind(beat.time_ms)
        .bind(monitor)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Admits one chunk, waiting while the inbox is at its budget so the import never outruns
/// transfer.
async fn admit(storage: &Storage, batch: BatchId, beats: &[Heartbeat]) -> anyhow::Result<()> {
    loop {
        match storage.admit(Some(batch.clone()), beats).await {
            Ok(_) => return Ok(()),
            Err(Refusal::Busy) => tokio::time::sleep(BUSY_WAIT).await,
            Err(refusal) => anyhow::bail!(
                "heartbeat storage refused chunk {}: {refusal}; free disk space and run the import again to resume",
                batch.id
            ),
        }
    }
}
