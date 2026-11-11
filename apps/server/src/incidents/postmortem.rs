//! An incident's postmortem: plain text the team writes once the incident is resolved. It stays
//! when the incident is reopened; pages show it only while the incident is resolved.

use sqlx::SqliteConnection;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::limits::INCIDENT_POSTMORTEM_MAX_CHARS;
use v1per_protocol::operations::incidents::Postmortem;

use super::{Row, text};
use crate::context::CommandCtx;
use crate::error::{ApiError, FieldErrors};

/// The postmortem of `incident`, if it has one.
pub(super) async fn load(
    conn: &mut SqliteConnection,
    incident: i64,
) -> sqlx::Result<Option<Postmortem>> {
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT postmortem, postmortem_at FROM incidents
         WHERE id = ? AND postmortem IS NOT NULL",
    )
    .bind(incident)
    .fetch_optional(conn)
    .await?;
    Ok(row.map(|(text, published_at)| Postmortem { text, published_at }))
}

/// Sets or, with empty `value`, removes the postmortem of a resolved incident. Unchanged text
/// keeps its time.
pub(super) async fn set(ctx: &mut CommandCtx, row: &Row, value: &str) -> Result<(), ApiError> {
    let value = text("postmortem", value, false, INCIDENT_POSTMORTEM_MAX_CHARS)?;
    if row.resolved_at.is_none() {
        return Err(FieldErrors::one(
            "postmortem",
            FieldErrorCode::InvalidValue,
            None,
        ));
    }
    let value = (!value.is_empty()).then_some(value);
    let now = ctx.now();
    sqlx::query(
        "UPDATE incidents
         SET postmortem_at = CASE WHEN ?1 IS NULL THEN NULL
                                  WHEN postmortem IS ?1 THEN postmortem_at ELSE ?2 END,
             postmortem = ?1, revision = revision + 1, updated_at = ?2
         WHERE id = ?3",
    )
    .bind(value)
    .bind(now)
    .bind(row.id)
    .execute(ctx.conn())
    .await?;
    Ok(())
}
