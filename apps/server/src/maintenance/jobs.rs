//! The job queue in `maintenance_jobs` and the runner that works through it, oldest first and
//! one at a time. The server runs it beside request handling and queues retention hourly; an
//! operator command without a server runs it until its own job finished.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::sync::watch;

use super::{backup, diagnostics, internal, retention};
use crate::init::{Exit, Failure};
use crate::state::AppState;

const POLL: Duration = Duration::from_secs(1);
const RETENTION_EVERY_MS: i64 = 60 * 60_000;
/// The first scheduled retention waits this long after start, so probe agents that were
/// cut off resend their unacknowledged batches before old batch IDs are forgotten.
const RETENTION_AFTER_START_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Job {
    Retention,
    RetentionDryRun,
    Backup { out: PathBuf, include_key: bool },
    Diagnostics,
}

impl Job {
    fn kind(&self) -> &'static str {
        match self {
            Job::Retention => "retention",
            Job::RetentionDryRun => "retention_dry_run",
            Job::Backup { .. } => "backup",
            Job::Diagnostics => "diagnostics",
        }
    }
}

pub async fn enqueue(pool: &SqlitePool, job: &Job, now_ms: i64) -> sqlx::Result<i64> {
    sqlx::query_scalar(
        "INSERT INTO maintenance_jobs (kind, args, state, created_at)
         VALUES (?, ?, 'queued', ?) RETURNING id",
    )
    .bind(job.kind())
    .bind(serde_json::to_string(job).expect("jobs serialize"))
    .bind(now_ms)
    .fetch_one(pool)
    .await
}

/// A finished job's result, or the failure it reported; `None` while it waits or runs.
pub async fn finished(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Result<Value, Failure>>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT state, result FROM maintenance_jobs WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let Some((state, result)) = row else {
        return Ok(None);
    };
    let result: Value = result
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|e| sqlx::Error::Decode(e.into()))?
        .unwrap_or(Value::Null);
    Ok(match state.as_str() {
        "done" => Some(Ok(result)),
        "failed" => Some(Err(failure_of(&result))),
        _ => None,
    })
}

fn failure_json(failure: &Failure) -> Value {
    json!({ "exit": failure.exit as u8, "code": failure.code, "message": failure.message })
}

fn failure_of(result: &Value) -> Failure {
    let exit = match result["exit"].as_u64() {
        Some(2) => Exit::Usage,
        Some(3) => Exit::Refused,
        Some(4) => Exit::StorageUnavailable,
        _ => Exit::Internal,
    };
    Failure::new(
        exit,
        result["code"].as_str().unwrap_or("internal").to_owned(),
        result["message"].as_str().unwrap_or_default().to_owned(),
    )
}

/// Runs queued jobs until `stop` changes. Started with the server.
pub async fn follow(state: AppState, mut stop: watch::Receiver<bool>) {
    if let Err(error) = abandon_interrupted(&state).await {
        tracing::error!(%error, "maintenance jobs could not be recovered");
    }
    while !*stop.borrow() {
        if let Err(error) = schedule_retention(&state).await {
            tracing::error!(%error, "retention could not be scheduled");
        }
        loop {
            match run_next(&state).await {
                Ok(true) if !*stop.borrow() => continue,
                Ok(_) => break,
                Err(error) => {
                    tracing::error!(%error, "maintenance job queue failed; retrying");
                    break;
                }
            }
        }
        tokio::select! {
            _ = tokio::time::sleep(POLL) => {}
            _ = stop.changed() => {}
        }
    }
}

/// Runs the queue until job `id` finished, for operator commands without a server.
pub async fn run_until(state: &AppState, id: i64) -> Result<Value, Failure> {
    abandon_interrupted(state).await.map_err(internal)?;
    loop {
        if let Some(result) = finished(&state.db.reader, id).await.map_err(internal)? {
            return result;
        }
        if !run_next(state).await.map_err(internal)? {
            tokio::time::sleep(POLL).await;
        }
    }
}

/// A job a stop interrupted fails: the operator asks again, and retention runs again within
/// the hour. Each retention step finds nothing to do when it already ran.
async fn abandon_interrupted(state: &AppState) -> sqlx::Result<()> {
    let interrupted = failure_json(&Failure::new(
        Exit::Internal,
        "interrupted",
        "the server stopped while this job ran; run the command again",
    ));
    sqlx::query(
        "UPDATE maintenance_jobs SET state = 'failed', result = ?, finished_at = ?
         WHERE state = 'running'",
    )
    .bind(interrupted.to_string())
    .bind(state.clock.now_ms())
    .execute(&state.db.writer)
    .await?;
    Ok(())
}

async fn schedule_retention(state: &AppState) -> sqlx::Result<()> {
    let now = state.clock.now_ms();
    if now < state.started_at + RETENTION_AFTER_START_MS {
        return Ok(());
    }
    let due: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS (SELECT 1 FROM maintenance_jobs WHERE kind = 'retention'
             AND (state IN ('queued', 'running') OR created_at > ?))",
    )
    .bind(now - RETENTION_EVERY_MS)
    .fetch_one(&state.db.reader)
    .await?;
    if due {
        enqueue(&state.db.writer, &Job::Retention, now).await?;
    }
    Ok(())
}

/// Claims and runs the oldest queued job. False when the queue is empty.
async fn run_next(state: &AppState) -> sqlx::Result<bool> {
    let claimed: Option<(i64, String)> = sqlx::query_as(
        "UPDATE maintenance_jobs SET state = 'running', started_at = ?
         WHERE id = (SELECT min(id) FROM maintenance_jobs WHERE state = 'queued')
         RETURNING id, args",
    )
    .bind(state.clock.now_ms())
    .fetch_optional(&state.db.writer)
    .await?;
    let Some((id, args)) = claimed else {
        return Ok(false);
    };
    let outcome = match serde_json::from_str::<Job>(&args) {
        Ok(job) => execute(state, &job).await,
        Err(error) => Err(internal(format!("unreadable job: {error}"))),
    };
    let (status, result) = match &outcome {
        Ok(result) => ("done", result.clone()),
        Err(failure) => {
            tracing::error!(job = id, code = %failure.code, message = %failure.message, "maintenance job failed");
            ("failed", failure_json(failure))
        }
    };
    sqlx::query("UPDATE maintenance_jobs SET state = ?, result = ?, finished_at = ? WHERE id = ?")
        .bind(status)
        .bind(result.to_string())
        .bind(state.clock.now_ms())
        .bind(id)
        .execute(&state.db.writer)
        .await?;
    Ok(true)
}

async fn execute(state: &AppState, job: &Job) -> Result<Value, Failure> {
    match job {
        Job::Retention => json_of(retention::run(state, false).await?),
        Job::RetentionDryRun => json_of(retention::run(state, true).await?),
        Job::Backup { out, include_key } => json_of(
            backup::take(
                backup::Source::Running(state),
                &state.secrets,
                out,
                *include_key,
            )
            .await?,
        ),
        Job::Diagnostics => {
            let mut conn = state.db.reader.acquire().await.map_err(internal)?;
            json_of(
                diagnostics::collect(state, &mut conn)
                    .await
                    .map_err(internal)?,
            )
        }
    }
}

fn json_of(result: impl Serialize) -> Result<Value, Failure> {
    serde_json::to_value(result).map_err(internal)
}
