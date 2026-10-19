//! `imports.*`: the import of a node and its inventory, for instance admins.

use sqlx::SqliteConnection;
use v1per_protocol::envelope::{Empty, Topic};
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::NodeId;
use v1per_protocol::operations::imports::{
    ImportArgs, ImportInventoryArgs, ImportInventoryItem, ImportInventoryPage, ImportList,
    ImportRun, ImportState, ImportSummary, ImportsHandlers,
};
use v1per_protocol::registry::Snapshot;

use super::state;
use crate::context::QueryCtx;
use crate::error::{ApiError, FieldErrors};
use crate::state::AppState;

const INVENTORY_PAGE: u32 = 100;
const INVENTORY_PAGE_MAX: u32 = 500;

/// Every import, newest first.
pub(crate) async fn summaries(conn: &mut SqliteConnection) -> sqlx::Result<Vec<ImportSummary>> {
    type Row = (i64, String, String, String, i64, Option<i64>, Option<bool>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT r.node_id, n.name, r.source, r.state, r.started_at, r.finished_at,
                json_extract(r.manifest, '$.reconciled')
         FROM import_runs r JOIN nodes n ON n.id = r.node_id
         ORDER BY r.started_at DESC, r.id DESC",
    )
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(node, node_name, source, state, started_at, finished_at, reconciled)| ImportSummary {
                node: NodeId::new(node),
                node_name,
                source,
                state: if state == "complete" {
                    ImportState::Complete
                } else {
                    ImportState::Running
                },
                started_at,
                finished_at,
                reconciled,
            },
        )
        .collect())
}

impl ImportsHandlers for AppState {
    async fn imports_list(
        &self,
        ctx: &mut QueryCtx,
        _args: Empty,
    ) -> Result<Snapshot<ImportList>, ApiError> {
        ctx.require_instance_admin().await?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: ImportList {
                imports: summaries(ctx.conn()).await?,
            },
        })
    }

    async fn imports_get(
        &self,
        ctx: &mut QueryCtx,
        args: ImportArgs,
    ) -> Result<Snapshot<ImportRun>, ApiError> {
        ctx.require_instance_admin().await?;
        let run = state::run_for(ctx.conn(), args.node)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let steps = state::progress(ctx.conn(), run.id).await?;
        let manifest = run
            .manifest
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(ApiError::internal)?;
        Ok(Snapshot {
            topic: Topic::node(args.node),
            data: ImportRun {
                node: args.node,
                state: run.state(),
                source: run.source,
                started_at: run.started_at,
                finished_at: run.finished_at,
                steps,
                manifest,
            },
        })
    }

    async fn imports_inventory(
        &self,
        ctx: &mut QueryCtx,
        args: ImportInventoryArgs,
    ) -> Result<Snapshot<ImportInventoryPage>, ApiError> {
        ctx.require_instance_admin().await?;
        let limit = args.limit.unwrap_or(INVENTORY_PAGE);
        if !(1..=INVENTORY_PAGE_MAX).contains(&limit) {
            return Err(FieldErrors::one(
                "limit",
                FieldErrorCode::InvalidValue,
                Some(INVENTORY_PAGE_MAX),
            ));
        }
        let after = match args.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor
                .parse::<i64>()
                .map_err(|_| FieldErrors::one("cursor", FieldErrorCode::InvalidValue, None))?,
        };
        let run = state::run_for(ctx.conn(), args.node)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let rows: Vec<(i64, String, String, String, String, i64, String)> = sqlx::query_as(
            "SELECT id, kind, label, reason, source_table, source_id, columns
             FROM import_inventory WHERE run_id = ? AND id > ? ORDER BY id LIMIT ?",
        )
        .bind(run.id)
        .bind(after)
        .bind(i64::from(limit) + 1)
        .fetch_all(ctx.conn())
        .await?;
        let more = rows.len() > limit as usize;
        let rows = &rows[..rows.len().min(limit as usize)];
        let next_cursor = more
            .then(|| rows.last().map(|row| row.0.to_string()))
            .flatten();
        let items = rows
            .iter()
            .map(|(_, kind, label, reason, table, source_id, columns)| {
                Ok(ImportInventoryItem {
                    kind: kind.clone(),
                    label: label.clone(),
                    reason: reason.clone(),
                    source_table: table.clone(),
                    source_id: *source_id,
                    columns: serde_json::from_str(columns).map_err(ApiError::internal)?,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic: Topic::node(args.node),
            data: ImportInventoryPage { items, next_cursor },
        })
    }
}
