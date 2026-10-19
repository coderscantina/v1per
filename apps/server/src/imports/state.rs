//! Import state in the control database (`0008_imports.sql`): one run per node, step
//! checkpoints, the Kuma rows behind created records, and the inventory.

use std::collections::BTreeMap;

use sqlx::SqliteConnection;
use v1per_protocol::ids::NodeId;
use v1per_protocol::operations::imports::{ImportState, ImportStep, ImportStepProgress};

use crate::db::now_ms;

#[derive(sqlx::FromRow)]
pub struct Run {
    pub id: i64,
    pub source: String,
    pub fingerprint: String,
    pub options: String,
    pub clock_ms: i64,
    state: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub manifest: Option<String>,
}

impl Run {
    pub fn state(&self) -> ImportState {
        if self.state == "complete" {
            ImportState::Complete
        } else {
            ImportState::Running
        }
    }
}

pub async fn run_for(conn: &mut SqliteConnection, node: NodeId) -> sqlx::Result<Option<Run>> {
    sqlx::query_as(
        "SELECT id, source, fingerprint, options, clock_ms, state, started_at, finished_at,
                manifest
         FROM import_runs WHERE node_id = ?",
    )
    .bind(node.get())
    .fetch_optional(conn)
    .await
}

/// The node of an unfinished import, if any, and its source.
pub async fn unfinished(conn: &mut SqliteConnection) -> sqlx::Result<Option<(NodeId, String)>> {
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT node_id, source FROM import_runs WHERE state = 'running' ORDER BY id LIMIT 1",
    )
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(node, source)| (NodeId::new(node), source)))
}

pub async fn start(
    conn: &mut SqliteConnection,
    node: NodeId,
    source: &str,
    fingerprint: &str,
    options: &str,
    clock_ms: i64,
) -> sqlx::Result<i64> {
    let now = now_ms();
    sqlx::query_scalar(
        "INSERT INTO import_runs (node_id, source, fingerprint, options, clock_ms, state,
                                  started_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'running', ?, ?) RETURNING id",
    )
    .bind(node.get())
    .bind(source)
    .bind(fingerprint)
    .bind(options)
    .bind(clock_ms)
    .bind(now)
    .bind(now)
    .fetch_one(conn)
    .await
}

pub async fn complete(
    conn: &mut SqliteConnection,
    run: i64,
    manifest: &str,
    finished_at: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE import_runs SET state = 'complete', manifest = ?, finished_at = ?, updated_at = ?
         WHERE id = ?",
    )
    .bind(manifest)
    .bind(finished_at)
    .bind(finished_at)
    .bind(run)
    .execute(conn)
    .await?;
    Ok(())
}

pub fn step_name(step: ImportStep) -> &'static str {
    match step {
        ImportStep::Control => "control",
        ImportStep::Stats => "stats",
        ImportStep::Heartbeats => "heartbeats",
        ImportStep::Reconcile => "reconcile",
    }
}

/// One step's checkpoint.
#[derive(Debug, Clone, Default)]
pub struct Checkpoint {
    pub cursor: Option<String>,
    pub chunks: i64,
    pub done: bool,
}

pub async fn checkpoint_of(
    conn: &mut SqliteConnection,
    run: i64,
    step: ImportStep,
) -> sqlx::Result<Checkpoint> {
    let row: Option<(Option<String>, i64, bool)> = sqlx::query_as(
        "SELECT cursor, chunks, done FROM import_steps WHERE run_id = ? AND step = ?",
    )
    .bind(run)
    .bind(step_name(step))
    .fetch_optional(conn)
    .await?;
    Ok(
        row.map_or_else(Checkpoint::default, |(cursor, chunks, done)| Checkpoint {
            cursor,
            chunks,
            done,
        }),
    )
}

/// What one committed chunk moves a step to, and what it added.
pub struct Advance<'a> {
    pub cursor: Option<&'a str>,
    pub chunks: i64,
    pub rows: i64,
    pub skipped: i64,
    pub done: bool,
}

pub async fn advance(
    conn: &mut SqliteConnection,
    run: i64,
    step: ImportStep,
    advance: Advance<'_>,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO import_steps (run_id, step, cursor, chunks, rows, skipped, done, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (run_id, step) DO UPDATE SET cursor = excluded.cursor,
             chunks = excluded.chunks, rows = rows + excluded.rows,
             skipped = skipped + excluded.skipped, done = excluded.done,
             updated_at = excluded.updated_at",
    )
    .bind(run)
    .bind(step_name(step))
    .bind(advance.cursor)
    .bind(advance.chunks)
    .bind(advance.rows)
    .bind(advance.skipped)
    .bind(advance.done)
    .bind(now_ms())
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn progress(
    conn: &mut SqliteConnection,
    run: i64,
) -> sqlx::Result<Vec<ImportStepProgress>> {
    let rows: Vec<(String, i64, i64, i64, bool)> = sqlx::query_as(
        "SELECT step, chunks, rows, skipped, done FROM import_steps WHERE run_id = ?",
    )
    .bind(run)
    .fetch_all(conn)
    .await?;
    let mut steps = Vec::new();
    for step in [
        ImportStep::Control,
        ImportStep::Stats,
        ImportStep::Heartbeats,
        ImportStep::Reconcile,
    ] {
        let found = rows.iter().find(|row| row.0 == step_name(step));
        steps.push(ImportStepProgress {
            step,
            chunks: found.map_or(0, |row| row.1),
            rows: found.map_or(0, |row| row.2),
            skipped: found.map_or(0, |row| row.3),
            done: found.is_some_and(|row| row.4),
        });
    }
    Ok(steps)
}

/// The record a Kuma row of `kind` became.
pub async fn record_object(
    conn: &mut SqliteConnection,
    run: i64,
    kind: &str,
    source_id: i64,
    target_id: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO import_objects (run_id, kind, source_id, target_id) VALUES (?, ?, ?, ?)",
    )
    .bind(run)
    .bind(kind)
    .bind(source_id)
    .bind(target_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Kuma ID to v1per row ID of every record of `kind` the run created.
pub async fn objects(
    conn: &mut SqliteConnection,
    run: i64,
    kind: &str,
) -> sqlx::Result<BTreeMap<i64, i64>> {
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT source_id, target_id FROM import_objects WHERE run_id = ? AND kind = ?",
    )
    .bind(run)
    .bind(kind)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(source, target)| Some((source, target.parse().ok()?)))
        .collect())
}
