//! DuckDB heartbeat store: raw heartbeats, minute, hour and day rollups, imported Kuma
//! rollups, and the applied-batch ledger.

use std::path::Path;
use std::time::{Duration, Instant};

use duckdb::types::{TimeUnit, Value};
use duckdb::{Config, Connection, OptionalExt, params};

use crate::inbox::Batch;
use crate::record::{INBOX_FORMAT, MonitorId};
use crate::{Budgets, Error, Resolution, Watermark, failpoint};

/// Schema version, recorded in `schema_info` and in every ledger entry.
pub const HEARTBEAT_SCHEMA: i64 = 1;

const ROLLUP_COLUMNS: &str = "
    monitor_id BIGINT NOT NULL,
    -- NULL: the monitor-level series (evaluator entries and server-decided heartbeats)
    probe_id BIGINT,
    bucket TIMESTAMP NOT NULL,
    up INTEGER NOT NULL,
    down INTEGER NOT NULL,
    pending INTEGER NOT NULL,
    maintenance INTEGER NOT NULL,
    -- latency of up heartbeats only, as Kuma averages ping over up beats
    latency_min UINTEGER,
    latency_max UINTEGER,
    latency_sum BIGINT NOT NULL,
    latency_count INTEGER NOT NULL";

fn schema() -> String {
    let rollups: String = Resolution::ALL
        .iter()
        .map(|r| {
            format!(
                "CREATE TABLE IF NOT EXISTS {} ({ROLLUP_COLUMNS});\n",
                r.table()
            )
        })
        .collect();
    format!(
        r#"
CREATE TABLE IF NOT EXISTS schema_info (heartbeat_schema INTEGER NOT NULL);
-- One row per transfer batch, committed with the rows it wrote. No primary key: batch IDs
-- grow with insertion order, so zone maps answer the lookup without an index.
CREATE TABLE IF NOT EXISTS applied_batches (
    batch_id BIGINT NOT NULL,
    first_seq BIGINT NOT NULL,
    last_seq BIGINT NOT NULL,
    records BIGINT NOT NULL,
    heartbeat_schema INTEGER NOT NULL,
    inbox_format INTEGER NOT NULL,
    applied_at TIMESTAMP NOT NULL DEFAULT current_timestamp
);
CREATE TABLE IF NOT EXISTS heartbeat (
    monitor_id BIGINT NOT NULL,
    -- NULL for results the server decides without a probe (push, group, manual)
    probe_id BIGINT,
    time TIMESTAMP NOT NULL,
    status UTINYINT NOT NULL,
    latency_ms UINTEGER,
    message VARCHAR NOT NULL,
    retries UINTEGER NOT NULL,
    http_status USMALLINT,
    cert_expiry TIMESTAMP
);
{rollups}
-- Kuma stat_minutely, stat_hourly and stat_daily rows as imported, `resolution` 1, 2, 3.
-- They count as the monitor-level series.
-- `bucket` is Kuma's bucket start in UTC; `ping` is Kuma's average over up beats.
CREATE TABLE IF NOT EXISTS kuma_stats (
    resolution UTINYINT NOT NULL,
    monitor_id BIGINT NOT NULL,
    bucket TIMESTAMP NOT NULL,
    ping DOUBLE,
    ping_min DOUBLE,
    ping_max DOUBLE,
    up INTEGER NOT NULL,
    down INTEGER NOT NULL,
    extras VARCHAR
);
"#
    )
}

pub(crate) fn open(path: &Path, spill: &Path, budgets: &Budgets) -> Result<Connection, Error> {
    let config = Config::default()
        .threads(budgets.engine_threads as i64)?
        .max_memory(&format!("{}B", budgets.engine_memory_bytes))?
        .with("temp_directory", spill.to_str().expect("utf-8 data path"))?
        .with("preserve_insertion_order", "false")?
        .enable_autoload_extension(false)?;
    let conn = Connection::open_with_flags(path, config)?;
    conn.execute_batch(&schema())?;
    let stored: Option<i64> = conn
        .query_row("SELECT heartbeat_schema FROM schema_info", [], |r| r.get(0))
        .optional()?;
    match stored {
        Some(HEARTBEAT_SCHEMA) => {}
        Some(other) => {
            return Err(Error::Incompatible(format!(
                "heartbeats.duckdb has schema {other}, this release {HEARTBEAT_SCHEMA}"
            )));
        }
        None => {
            conn.execute("INSERT INTO schema_info VALUES (?)", [HEARTBEAT_SCHEMA])?;
        }
    }
    // Transfer stages each batch here; temporary tables belong to the writer connection only.
    conn.execute_batch(
        "CREATE TEMP TABLE staged AS SELECT * FROM heartbeat LIMIT 0;
         CREATE TEMP TABLE staged_status (
             monitor_id BIGINT NOT NULL, time TIMESTAMP NOT NULL,
             status UTINYINT NOT NULL, latency_ms UINTEGER
         );",
    )?;
    Ok(conn)
}

/// Opens a heartbeat file that the server does not hold, such as a backup copy, with a
/// small budget and without creating anything.
pub(crate) fn open_file(path: &Path) -> duckdb::Result<Connection> {
    let config = Config::default()
        .threads(1)?
        .max_memory("256MB")?
        .enable_autoload_extension(false)?;
    Connection::open_with_flags(path, config)
}

/// The stored schema version; `None` before the first open created it.
pub(crate) fn stored_schema(conn: &Connection) -> duckdb::Result<Option<i64>> {
    let exists: bool = conn.query_row(
        "SELECT count(*) > 0 FROM duckdb_tables() WHERE table_name = 'schema_info'",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(None);
    }
    conn.query_row("SELECT heartbeat_schema FROM schema_info", [], |r| r.get(0))
        .optional()
}

/// The last applied batch with its last sequence, and the inbox rows of every batch applied.
pub(crate) fn ledger(conn: &Connection) -> duckdb::Result<(Watermark, i64)> {
    let records: i64 = conn.query_row(
        "SELECT coalesce(sum(records), 0)::BIGINT FROM applied_batches",
        [],
        |r| r.get(0),
    )?;
    Ok((watermark(conn)?, records))
}

pub(crate) fn watermark(conn: &Connection) -> duckdb::Result<Watermark> {
    conn.query_row(
        "SELECT batch_id, last_seq FROM applied_batches ORDER BY batch_id DESC LIMIT 1",
        [],
        |row| {
            Ok(Watermark {
                batch: row.get(0)?,
                sequence: row.get(1)?,
            })
        },
    )
    .optional()
    .map(Option::unwrap_or_default)
}

pub(crate) fn ts(ms: i64) -> Value {
    Value::Timestamp(TimeUnit::Millisecond, ms)
}

/// Applies a stable batch exactly once: raw heartbeats, rollup updates (from heartbeats and
/// monitor-level entries) and the ledger entry commit in one DuckDB transaction. A batch
/// already in the ledger is not applied again. Returns the new watermark and the time the
/// rollup merges took.
pub(crate) fn apply(conn: &mut Connection, batch: &Batch) -> Result<(Watermark, Duration), Error> {
    let tx = conn.transaction()?;
    let membership = [
        batch.first_seq,
        batch.last_seq,
        batch.rows.len() as i64,
        HEARTBEAT_SCHEMA,
        INBOX_FORMAT,
    ];
    let existing = tx
        .query_row(
            "SELECT first_seq, last_seq, records, heartbeat_schema, inbox_format
             FROM applied_batches WHERE batch_id = ?",
            [batch.id],
            |r| {
                Ok([
                    r.get::<_, i64>(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                ])
            },
        )
        .optional()?;
    if let Some(entry) = existing {
        if entry != membership {
            return Err(Error::Incompatible(format!(
                "batch {} ledger entry {entry:?} differs from inbox membership {membership:?}",
                batch.id
            )));
        }
        drop(tx);
        return Ok((watermark(conn)?, Duration::ZERO));
    }
    let previous = watermark(&tx)?;
    if batch.id <= previous.batch {
        return Err(Error::Incompatible(format!(
            "batch {} is older than applied batch {} but missing from the ledger",
            batch.id, previous.batch
        )));
    }

    let mut staged = tx.appender_to_catalog_and_db("staged", "temp", "main")?;
    for heartbeat in batch.rows.iter().flat_map(|(_, p)| &p.heartbeats) {
        staged.append_row(params![
            heartbeat.monitor.0,
            heartbeat.probe.map(|p| p.0),
            ts(heartbeat.time_ms),
            heartbeat.status as u8,
            heartbeat.latency_ms,
            heartbeat.message,
            heartbeat.retries,
            heartbeat.http_status,
            heartbeat.cert_expiry_ms.map(ts),
        ])?;
    }
    staged.flush()?;
    drop(staged);
    let mut statuses = tx.appender_to_catalog_and_db("staged_status", "temp", "main")?;
    for status in batch.rows.iter().flat_map(|(_, p)| &p.statuses) {
        statuses.append_row(params![
            status.monitor.0,
            ts(status.time_ms),
            status.status as u8,
            status.latency_ms,
        ])?;
    }
    statuses.flush()?;
    drop(statuses);
    tx.execute_batch("INSERT INTO heartbeat SELECT * FROM temp.staged")?;
    failpoint::hit("transfer.during_apply");
    let merging = Instant::now();
    for resolution in Resolution::ALL {
        merge_rollup(&tx, resolution)?;
    }
    let rollups = merging.elapsed();
    tx.execute_batch("DELETE FROM temp.staged; DELETE FROM temp.staged_status;")?;
    tx.execute(
        "INSERT INTO applied_batches (batch_id, first_seq, last_seq, records, heartbeat_schema, inbox_format)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            batch.id,
            batch.first_seq,
            batch.last_seq,
            batch.rows.len() as i64,
            HEARTBEAT_SCHEMA,
            INBOX_FORMAT
        ],
    )?;
    tx.commit()?;
    let applied = Watermark {
        batch: batch.id,
        sequence: batch.last_seq,
    };
    Ok((applied, rollups))
}

/// Adds the staged heartbeats and monitor-level entries to one rollup table: buckets that
/// exist are updated, new ones inserted. A heartbeat with probe NULL is the monitor-level
/// entry of a server-decided monitor and counts once, like an evaluator entry. The bucket range bound lets zone maps skip the rollup rows the batch can't touch.
fn merge_rollup(conn: &Connection, resolution: Resolution) -> duckdb::Result<()> {
    let table = resolution.table();
    conn.execute_batch(&format!(
        "CREATE OR REPLACE TEMP TABLE delta AS
         SELECT monitor_id, probe_id, date_trunc('{unit}', time)::TIMESTAMP AS bucket,
                count(*) FILTER (WHERE status = 1)::INTEGER AS up,
                count(*) FILTER (WHERE status = 0)::INTEGER AS down,
                count(*) FILTER (WHERE status = 2)::INTEGER AS pending,
                count(*) FILTER (WHERE status = 3)::INTEGER AS maintenance,
                min(latency_ms) FILTER (WHERE status = 1) AS latency_min,
                max(latency_ms) FILTER (WHERE status = 1) AS latency_max,
                coalesce(sum(latency_ms) FILTER (WHERE status = 1), 0)::BIGINT AS latency_sum,
                count(latency_ms) FILTER (WHERE status = 1)::INTEGER AS latency_count
         FROM (SELECT monitor_id, probe_id, time, status, latency_ms FROM temp.staged
               UNION ALL
               SELECT monitor_id, NULL, time, status, latency_ms FROM temp.staged_status)
         GROUP BY ALL",
        unit = resolution.unit()
    ))?;
    let Some((from, to)) = conn.query_row(
        "SELECT epoch_ms(min(bucket)), epoch_ms(max(bucket)) FROM temp.delta",
        [],
        |r| Ok(r.get::<_, Option<i64>>(0)?.zip(r.get::<_, Option<i64>>(1)?)),
    )?
    else {
        return Ok(());
    };
    conn.execute(
        &format!(
            "UPDATE {table} AS r SET
                up = r.up + d.up, down = r.down + d.down, pending = r.pending + d.pending,
                maintenance = r.maintenance + d.maintenance,
                latency_min = least(r.latency_min, d.latency_min),
                latency_max = greatest(r.latency_max, d.latency_max),
                latency_sum = r.latency_sum + d.latency_sum,
                latency_count = r.latency_count + d.latency_count
             FROM temp.delta AS d
             WHERE r.bucket BETWEEN ? AND ? AND r.monitor_id = d.monitor_id
               AND r.probe_id IS NOT DISTINCT FROM d.probe_id AND r.bucket = d.bucket"
        ),
        params![ts(from), ts(to)],
    )?;
    conn.execute(
        &format!(
            "INSERT INTO {table}
             SELECT d.monitor_id, d.probe_id, d.bucket, d.up, d.down, d.pending, d.maintenance,
                    d.latency_min, d.latency_max, d.latency_sum, d.latency_count
             FROM temp.delta AS d
             WHERE NOT EXISTS (SELECT 1 FROM {table} AS r
                               WHERE r.bucket BETWEEN ? AND ?
                                 AND r.monitor_id = d.monitor_id
                                 AND r.probe_id IS NOT DISTINCT FROM d.probe_id
                                 AND r.bucket = d.bucket)"
        ),
        params![ts(from), ts(to)],
    )?;
    Ok(())
}

/// Deletes up to one day of rows older than `cutoff_ms` from `table`, oldest first, so the
/// writer is released between steps. Returns the rows deleted; zero means none are left.
pub(crate) fn expire_step(
    conn: &Connection,
    table: &str,
    column: &str,
    cutoff_ms: i64,
) -> duckdb::Result<u64> {
    const DAY_MS: i64 = 86_400_000;
    let oldest: Option<i64> = conn.query_row(
        &format!("SELECT epoch_ms(min({column})) FROM {table} WHERE {column} < ?"),
        [ts(cutoff_ms)],
        |r| r.get(0),
    )?;
    let Some(oldest) = oldest else { return Ok(0) };
    let until = cutoff_ms.min(oldest + DAY_MS);
    Ok(conn.execute(
        &format!("DELETE FROM {table} WHERE {column} < ?"),
        [ts(until)],
    )? as u64)
}

/// Rows of `table` older than `cutoff_ms`: what [`expire_step`] would delete.
pub(crate) fn count_before(
    conn: &Connection,
    table: &str,
    column: &str,
    cutoff_ms: i64,
) -> duckdb::Result<u64> {
    conn.query_row(
        &format!("SELECT count(*) FROM {table} WHERE {column} < ?"),
        [ts(cutoff_ms)],
        |r| r.get(0),
    )
}

/// One imported Kuma `stat_*` row, as Kuma stored it.
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

/// Replaces the monitor's imported rows of `resolution` in the span `rows` cover, so a chunk
/// imported again after an interruption is not counted twice.
pub(crate) fn import_kuma_stats(
    conn: &mut Connection,
    monitor: MonitorId,
    resolution: Resolution,
    rows: &[KumaStat],
) -> duckdb::Result<()> {
    let (Some(from), Some(to)) = (
        rows.iter().map(|r| r.bucket_ms).min(),
        rows.iter().map(|r| r.bucket_ms).max(),
    ) else {
        return Ok(());
    };
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM kuma_stats WHERE resolution = ? AND monitor_id = ? AND bucket BETWEEN ? AND ?",
        params![resolution.kuma_code(), monitor.0, ts(from), ts(to)],
    )?;
    let mut appender = tx.appender("kuma_stats")?;
    for row in rows {
        appender.append_row(params![
            resolution.kuma_code(),
            monitor.0,
            ts(row.bucket_ms),
            row.ping,
            row.ping_min,
            row.ping_max,
            row.up,
            row.down,
            row.extras
        ])?;
    }
    appender.flush()?;
    drop(appender);
    tx.commit()
}
