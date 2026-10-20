//! The operational state behind `v1per diagnostics` and `instance.operations`: versions, file
//! sizes, disk, inbox and transfer, probes, the outbox, retention, jobs and imports. Built
//! from counts, sizes, states and names, never from credentials or check messages.

use std::fmt::Write;
use std::path::Path;

use chrono::{SecondsFormat, TimeZone, Utc};
use sqlx::SqliteConnection;
use v1per_protocol::operations::instance::{
    DataFile, HeartbeatHealth, InboxHealth, InstanceOperations, MaintenanceJob, MaintenanceJobKind,
    MaintenanceJobState, OutboxHealth, RetentionCounts, RetentionRun, RetentionState, StorageUsage,
};
use v1per_protocol::operations::probes::ProbeStatus;
use v1per_storage::{HEARTBEATS_FILE, HEARTBEATS_WAL_FILE, INBOX_FILE, Retention};

use super::versions;
use crate::db::CONTROL_DB_FILE;
use crate::state::AppState;

/// Maintenance jobs listed, newest first.
const JOBS_LISTED: i64 = 20;
const DAY_MS: i64 = 86_400_000;
const FILES: [&str; 6] = [
    CONTROL_DB_FILE,
    "control.sqlite-wal",
    INBOX_FILE,
    "inbox.sqlite-wal",
    HEARTBEATS_FILE,
    HEARTBEATS_WAL_FILE,
];

pub async fn collect(
    state: &AppState,
    conn: &mut SqliteConnection,
) -> anyhow::Result<InstanceOperations> {
    let now = state.clock.now_ms();
    let dir = &state.config.data_dir;
    let storage = &state.storage;
    let health = storage.health();
    let disk = v1per_storage::disk(dir)?;
    let mut files: Vec<DataFile> = FILES
        .iter()
        .filter_map(|name| {
            let bytes = std::fs::metadata(dir.join(name)).ok()?.len();
            Some(DataFile {
                name: (*name).to_owned(),
                bytes,
            })
        })
        .collect();
    files.sort_by_key(|file| std::cmp::Reverse(file.bytes));

    let (pending, retrying, oldest_pending_at, failed_last_day): (i64, i64, Option<i64>, i64) =
        sqlx::query_as(
            "SELECT count(*) FILTER (WHERE state = 'pending'),
                    count(*) FILTER (WHERE state = 'pending' AND attempts > 0),
                    min(created_at) FILTER (WHERE state = 'pending'),
                    count(*) FILTER (WHERE state = 'failed' AND finished_at >= ?)
             FROM notification_outbox",
        )
        .bind(now - DAY_MS)
        .fetch_one(&mut *conn)
        .await?;

    type JobRow = (
        String,
        String,
        i64,
        Option<i64>,
        Option<i64>,
        Option<String>,
    );
    let rows: Vec<JobRow> = sqlx::query_as(
        "SELECT kind, state, created_at, started_at, finished_at,
                CASE WHEN state = 'failed' THEN json_extract(result, '$.code') END
         FROM maintenance_jobs ORDER BY id DESC LIMIT ?",
    )
    .bind(JOBS_LISTED)
    .fetch_all(&mut *conn)
    .await?;
    let jobs = rows
        .into_iter()
        .map(
            |(kind, job_state, created_at, started_at, finished_at, failure)| {
                Ok(MaintenanceJob {
                    kind: serde_json::from_value(kind.into())?,
                    state: serde_json::from_value(job_state.into())?,
                    created_at,
                    started_at,
                    finished_at,
                    failure,
                })
            },
        )
        .collect::<serde_json::Result<_>>()?;
    let last_run: Option<(i64, String)> = sqlx::query_as(
        "SELECT finished_at, result FROM maintenance_jobs
         WHERE kind = 'retention' AND state = 'done' ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&mut *conn)
    .await?;
    let last_run = last_run
        .map(|(finished_at, result)| {
            Ok::<_, serde_json::Error>(RetentionRun {
                finished_at,
                removed: serde_json::from_str(&result)?,
            })
        })
        .transpose()?;
    let retention = Retention::default();
    let days = |keep: std::time::Duration| (keep.as_secs() / 86_400) as u32;

    Ok(InstanceOperations {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        generated_at: now,
        schema: versions::release(),
        storage: StorageUsage {
            files,
            data_dir_bytes: dir_bytes(dir),
            disk_free_bytes: disk.free_bytes,
            disk_total_bytes: disk.total_bytes,
            disk_reserve_bytes: storage.budgets().disk_reserve_bytes,
        },
        inbox: InboxHealth {
            rows: health.inbox_rows,
            entries: health.inbox_entries,
            bytes: health.inbox_bytes,
            oldest_age_ms: storage.inbox_age().await?.as_millis() as u64,
        },
        heartbeats: HeartbeatHealth {
            watermark_batch: health.watermark.batch,
            watermark_sequence: health.watermark.sequence,
            transfer_failures: health.transfer_failures,
            transfer_failing_since: health.transfer_failing_since_ms,
        },
        probes: crate::probes::registry::all(state, conn).await?,
        outbox: OutboxHealth {
            pending: pending as u64,
            retrying: retrying as u64,
            oldest_pending_at,
            failed_last_day: failed_last_day as u64,
        },
        retention: RetentionState {
            raw_days: days(retention.raw),
            minute_rollup_days: retention.minute_rollups.map(days),
            last_run,
        },
        jobs,
        imports: crate::imports::api::summaries(conn).await?,
    })
}

/// Bytes of every file below `dir`. Files that vanish while it counts are skipped.
fn dir_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_bytes(&entry.path()),
            _ => entry.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// The text `v1per diagnostics` prints without `--json`.
pub fn render(ops: &InstanceOperations) -> String {
    let mut out = String::new();
    let schema = ops.schema;
    let storage = &ops.storage;
    let _ = writeln!(
        out,
        "V1Per {} at {}, schema control {} inbox {} heartbeats {}",
        ops.version,
        time(ops.generated_at),
        schema.control,
        schema.inbox,
        schema.heartbeats
    );
    let _ = writeln!(
        out,
        "\nDisk: {} free of {}, {} kept in reserve",
        size(storage.disk_free_bytes),
        size(storage.disk_total_bytes),
        size(storage.disk_reserve_bytes)
    );
    let _ = writeln!(out, "Data directory: {}", size(storage.data_dir_bytes));
    for file in &storage.files {
        let _ = writeln!(out, "  {:<22} {}", file.name, size(file.bytes));
    }
    let inbox = ops.inbox;
    let _ = writeln!(
        out,
        "\nInbox: {} admissions with {} results, {}, oldest waiting {:.1} s",
        inbox.rows,
        inbox.entries,
        size(inbox.bytes),
        inbox.oldest_age_ms as f64 / 1000.0
    );
    let beats = ops.heartbeats;
    let transfer = match beats.transfer_failing_since {
        Some(since) => format!("FAILING since {}", time(since)),
        None => "working".to_owned(),
    };
    let _ = writeln!(
        out,
        "Heartbeats: watermark batch {} sequence {}; transfer {transfer}, {} failed attempts since start",
        beats.watermark_batch, beats.watermark_sequence, beats.transfer_failures
    );
    let _ = writeln!(out, "\nProbes:");
    for probe in &ops.probes {
        let status = match probe.status {
            ProbeStatus::Waiting => "waiting for its agent",
            ProbeStatus::Online => "online",
            ProbeStatus::Offline => "OFFLINE",
            ProbeStatus::Revoked => "revoked",
        };
        let seen = probe
            .last_seen_at
            .map(|at| format!(", last seen {}", time(at)))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  {} ({status}{seen}): {} checks, {} late, {} buffered, {} dropped",
            probe.name,
            probe.checks_started,
            probe.checks_late,
            probe.buffered_results,
            probe.dropped_results
        );
    }
    let outbox = ops.outbox;
    let oldest = outbox
        .oldest_pending_at
        .map(|at| format!(", oldest queued {}", time(at)))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "\nOutbox: {} pending ({} retrying{oldest}), {} failed in the last day",
        outbox.pending, outbox.retrying, outbox.failed_last_day
    );
    let retention = ops.retention;
    let minutes = match retention.minute_rollup_days {
        Some(days) => format!("{days} days"),
        None => "forever".to_owned(),
    };
    let _ = writeln!(
        out,
        "Retention: raw heartbeats {} days, minute rollups {minutes}",
        retention.raw_days
    );
    match retention.last_run {
        Some(run) => {
            let _ = writeln!(
                out,
                "  last run {}: {}",
                time(run.finished_at),
                counts(&run.removed)
            );
        }
        None => {
            let _ = writeln!(out, "  no run yet");
        }
    }
    if !ops.jobs.is_empty() {
        let _ = writeln!(out, "\nJobs, newest first:");
    }
    for job in &ops.jobs {
        let kind = match job.kind {
            MaintenanceJobKind::Retention => "retention",
            MaintenanceJobKind::RetentionDryRun => "retention dry run",
            MaintenanceJobKind::Backup => "backup",
            MaintenanceJobKind::Diagnostics => "diagnostics",
        };
        let state = match job.state {
            MaintenanceJobState::Queued => "queued".to_owned(),
            MaintenanceJobState::Running => "running".to_owned(),
            MaintenanceJobState::Done => "done".to_owned(),
            MaintenanceJobState::Failed => {
                format!("FAILED {}", job.failure.as_deref().unwrap_or("internal"))
            }
        };
        let _ = writeln!(out, "  {} {kind}: {state}", time(job.created_at));
    }
    if !ops.imports.is_empty() {
        let _ = writeln!(out, "\nImports:");
    }
    for import in &ops.imports {
        let _ = writeln!(
            out,
            "  {} into {}: {:?} since {}",
            import.source,
            import.node_name,
            import.state,
            time(import.started_at)
        );
    }
    out
}

/// One line of what a retention run removed or would remove.
pub fn counts(counts: &RetentionCounts) -> String {
    format!(
        "{} heartbeats, {} minute rollups, {} probe batch IDs, {} deliveries, {} sessions, {} login steps, {} page sessions, {} sign-in links, {} command answers, {} jobs",
        counts.heartbeats,
        counts.minute_rollups,
        counts.agent_batches,
        counts.deliveries,
        counts.sessions,
        counts.login_challenges,
        counts.page_sessions,
        counts.page_email_codes,
        counts.idempotency_keys,
        counts.jobs
    )
}

fn time(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|at| at.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_else(|| ms.to_string())
}

/// Bytes in binary units with one decimal.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
