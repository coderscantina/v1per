//! Retention: heartbeats per [`Retention::default`] (raw 90 days, minute rollups kept until
//! Mike decides, decision 0002), probe batch IDs after a day, finished deliveries and
//! maintenance jobs after 30 days, and ended sessions, sign-in steps, page sessions, sign-in
//! links and command answers. Queries already ignore what ended; this gives the space back.
//! Every step finds nothing to do when it already ran, so an interrupted or failed run is
//! finished by the next one.

use std::path::Path;
use std::time::Duration;

use sqlx::Row;
use v1per_protocol::limits::{IDEMPOTENCY_WINDOW_MS, SESSION_IDLE_MS};
use v1per_protocol::operations::instance::RetentionCounts;
use v1per_storage::Retention;

use super::{internal, storage_failure};
use crate::init::{Exit, Failure};
use crate::state::AppState;

/// An agent resends a batch until it is acknowledged, so its ID only has to outlive a lost
/// acknowledgement. Kept a day: agents send a batch a second.
const AGENT_BATCH_KEEP: Duration = Duration::from_secs(86_400);
const DELIVERY_KEEP_MS: i64 = 30 * 86_400_000;
const JOB_KEEP_MS: i64 = 30 * 86_400_000;

pub async fn run(state: &AppState, dry_run: bool) -> Result<RetentionCounts, Failure> {
    let now = state.clock.now_ms();
    let dir = &state.config.data_dir;
    let mut counts = RetentionCounts::default();
    let sweeps: [(&mut u64, &str, &str, Vec<i64>); 7] = [
        (
            &mut counts.deliveries,
            "notification_outbox",
            "finished_at < ?",
            vec![now - DELIVERY_KEEP_MS],
        ),
        (
            &mut counts.sessions,
            "sessions",
            "expires_at <= ? OR last_seen_at <= ?",
            vec![now, now - SESSION_IDLE_MS],
        ),
        (
            &mut counts.login_challenges,
            "login_challenges",
            "expires_at <= ?",
            vec![now],
        ),
        (
            &mut counts.page_sessions,
            "page_sessions",
            "expires_at <= ?",
            vec![now],
        ),
        (
            &mut counts.page_email_codes,
            "page_email_codes",
            "expires_at <= ?",
            vec![now],
        ),
        (
            &mut counts.idempotency_keys,
            "idempotency_keys",
            "created_at <= ?",
            vec![now - IDEMPOTENCY_WINDOW_MS],
        ),
        (
            &mut counts.jobs,
            "maintenance_jobs",
            "state IN ('done', 'failed') AND finished_at < ?",
            vec![now - JOB_KEEP_MS],
        ),
    ];
    for (count, table, condition, binds) in sweeps {
        let sql = if dry_run {
            format!("SELECT count(*) FROM {table} WHERE {condition}")
        } else {
            format!("DELETE FROM {table} WHERE {condition}")
        };
        // Table and condition are the constants above; values go in as binds.
        let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            query = query.bind(bind);
        }
        let swept = if dry_run {
            query
                .fetch_one(&state.db.reader)
                .await
                .map(|row| row.get::<i64, _>(0) as u64)
        } else {
            query
                .execute(&state.db.writer)
                .await
                .map(|done| done.rows_affected())
        };
        *count = swept.map_err(|e| sql_failure(dir, e))?;
    }

    let storage = &state.storage;
    let expired = if dry_run {
        storage.expirable(Retention::default(), now).await
    } else {
        storage.expire(Retention::default(), now).await
    }
    .map_err(|e| storage_failure(dir, e))?;
    counts.heartbeats = expired.heartbeats;
    counts.minute_rollups = expired.minute_rollups;
    // Batch IDs carry the inbox's own receive time, not the application clock.
    let wall = crate::db::now_ms();
    counts.agent_batches = if dry_run {
        storage.prunable_agent_batches(AGENT_BATCH_KEEP, wall).await
    } else {
        storage.prune_agent_batches(AGENT_BATCH_KEEP, wall).await
    }
    .map_err(|e| storage_failure(dir, e))?;
    if !dry_run {
        tracing::info!(
            heartbeats = counts.heartbeats,
            minute_rollups = counts.minute_rollups,
            deliveries = counts.deliveries,
            sessions = counts.sessions,
            "retention"
        );
    }
    Ok(counts)
}

/// A control database error; a full disk (`SQLITE_FULL`) says so.
pub(crate) fn sql_failure(path: &Path, error: sqlx::Error) -> Failure {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("13") => Failure::new(
            Exit::StorageUnavailable,
            "disk_full",
            format!(
                "{}: {error}; free space and run the command again",
                path.display()
            ),
        ),
        sqlx::Error::Io(_) | sqlx::Error::Database(_) => Failure::storage(path, error),
        _ => internal(error),
    }
}
