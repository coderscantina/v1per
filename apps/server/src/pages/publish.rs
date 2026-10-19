//! Incident publication per page (decision 0004). Manual incidents go to the pages showing
//! their targets that their author manages incidents on. Auto-incidents follow each page's
//! mode: `draft` waits for a person, `auto_publish` publishes once the incident stayed open for
//! the page's grace, which [`run`] keeps time for.

use serde_json::json;
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{IncidentId, MonitorId, NodeId, PageId};
use v1per_protocol::operations::incidents::{IncidentPublication, IncidentPublications, Target};

use super::{Change, touch};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::Caller;
use crate::incidents::record_tree_ctx;
use crate::nodes::scope;
use crate::state::AppState;

/// The longest the timer sleeps, in case a wake was missed.
const IDLE_MS: i64 = 60 * 60_000;

/// Pages showing a target of `incident`, and pages it is published to: (id, node, slug,
/// title).
const CANDIDATES: &str = "SELECT p.id, p.node_id, p.slug, p.title FROM pages p
     WHERE p.id IN (
         SELECT pc.page_id FROM page_components pc JOIN incident_targets t
             ON t.monitor_id = pc.monitor_id OR t.component_id = pc.component_id
         WHERE t.incident_id = ?1
         UNION
         SELECT page_id FROM incident_publications WHERE incident_id = ?1
     )
     ORDER BY p.title, p.id";

/// A new auto-incident of `monitor`: publishes it at once on `auto_publish` pages without
/// grace and schedules it on the others. Returns the topics to wake.
pub(crate) async fn auto_opened(
    conn: &mut SqliteConnection,
    incident: IncidentId,
    monitor: MonitorId,
    at: i64,
) -> sqlx::Result<Vec<Topic>> {
    let pages: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT DISTINCT p.id, p.grace_minutes FROM pages p
         JOIN page_components pc ON pc.page_id = p.id
         WHERE pc.monitor_id = ? AND p.auto_incidents = 'auto_publish'",
    )
    .bind(monitor.get())
    .fetch_all(&mut *conn)
    .await?;
    for (page, grace) in pages {
        if grace == 0 {
            insert(conn, incident, PageId::new(page), at).await?;
        } else {
            sqlx::query(
                "INSERT OR IGNORE INTO incident_auto_publications (incident_id, page_id, due_at)
                 VALUES (?, ?, ?)",
            )
            .bind(incident.get())
            .bind(page)
            .bind(at + grace * 60_000)
            .execute(&mut *conn)
            .await?;
        }
    }
    touch(conn, Change::Incident(incident), at).await
}

/// An auto-incident resolved: one still waiting out a grace is not published any more.
pub(crate) async fn auto_resolved(
    conn: &mut SqliteConnection,
    incident: IncidentId,
    at: i64,
) -> sqlx::Result<Vec<Topic>> {
    sqlx::query("DELETE FROM incident_auto_publications WHERE incident_id = ?")
        .bind(incident.get())
        .execute(&mut *conn)
        .await?;
    touch(conn, Change::Incident(incident), at).await
}

async fn insert(
    conn: &mut SqliteConnection,
    incident: IncidentId,
    page: PageId,
    at: i64,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO incident_publications (incident_id, page_id, published_at)
         VALUES (?, ?, ?)",
    )
    .bind(incident.get())
    .bind(page.get())
    .bind(at)
    .execute(conn)
    .await?;
    Ok(())
}

async fn manages_incidents(
    conn: &mut SqliteConnection,
    caller: &Caller,
    node: NodeId,
) -> Result<bool, ApiError> {
    match scope::resolve(conn, caller, node).await {
        Ok(scope) => Ok(scope.can(Capability::IncidentsManage)),
        Err(error) if error.code() == v1per_protocol::errors::ErrorCode::Internal => Err(error),
        Err(_) => Ok(false),
    }
}

/// Publishes a new manual incident to the pages showing one of `targets` on which the author
/// manages incidents. The incident's `record` then tells those pages.
pub(crate) async fn publish_defaults(
    ctx: &mut CommandCtx,
    incident: IncidentId,
    targets: &[Target],
) -> Result<(), ApiError> {
    if targets.is_empty() {
        return Ok(());
    }
    let pages: Vec<(i64, i64, String, String)> = sqlx::query_as(CANDIDATES)
        .bind(incident.get())
        .fetch_all(ctx.conn())
        .await?;
    let caller = ctx.caller().clone();
    let now = ctx.now();
    for (page, node, ..) in pages {
        if manages_incidents(ctx.conn(), &caller, NodeId::new(node)).await? {
            insert(ctx.conn(), incident, PageId::new(page), now).await?;
        }
    }
    Ok(())
}

/// `incidents.publish` and `incidents.unpublish`.
pub(crate) async fn publish(
    ctx: &mut CommandCtx,
    incident: IncidentId,
    page: PageId,
    published: bool,
) -> Result<IncidentPublications, ApiError> {
    let node: i64 = sqlx::query_scalar("SELECT node_id FROM incidents WHERE id = ?")
        .bind(incident.get())
        .fetch_optional(ctx.conn())
        .await?
        .ok_or_else(ApiError::not_found)?;
    let node = NodeId::new(node);
    ctx.node_scope(node, Capability::IncidentsManage).await?;
    let row = super::load(ctx.conn(), page).await?;
    ctx.node_scope(row.node(), Capability::IncidentsManage)
        .await?;
    if published {
        let shows: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM page_components pc JOIN incident_targets t
                 ON t.monitor_id = pc.monitor_id OR t.component_id = pc.component_id
             WHERE t.incident_id = ? AND pc.page_id = ? LIMIT 1",
        )
        .bind(incident.get())
        .bind(page.get())
        .fetch_optional(ctx.conn())
        .await?;
        if shows.is_none() {
            return Err(FieldErrors::one("page", FieldErrorCode::InvalidValue, None));
        }
        let now = ctx.now();
        insert(ctx.conn(), incident, page, now).await?;
    } else {
        sqlx::query("DELETE FROM incident_publications WHERE incident_id = ? AND page_id = ?")
            .bind(incident.get())
            .bind(page.get())
            .execute(ctx.conn())
            .await?;
    }
    // A person decided for this page, so a pending auto-publication no longer applies.
    sqlx::query("DELETE FROM incident_auto_publications WHERE incident_id = ? AND page_id = ?")
        .bind(incident.get())
        .bind(page.get())
        .execute(ctx.conn())
        .await?;
    ctx.record_also(&Topic::page(page), "page.incident_changed", json!({}))
        .await?;
    record_tree_ctx(
        ctx,
        Topic::incidents,
        node,
        "incident.updated",
        json!({ "incident": incident }),
    )
    .await?;
    let caller = ctx.caller().clone();
    list(ctx.conn(), &caller, incident).await
}

/// `incidents.publications`.
pub(crate) async fn publications(
    ctx: &mut QueryCtx,
    incident: IncidentId,
) -> Result<IncidentPublications, ApiError> {
    let caller = ctx.caller().clone();
    list(ctx.conn(), &caller, incident).await
}

async fn list(
    conn: &mut SqliteConnection,
    caller: &Caller,
    incident: IncidentId,
) -> Result<IncidentPublications, ApiError> {
    let candidates: Vec<(i64, i64, String, String)> = sqlx::query_as(CANDIDATES)
        .bind(incident.get())
        .fetch_all(&mut *conn)
        .await?;
    let mut pages = Vec::new();
    for (page, node, slug, title) in candidates {
        if scope::resolve(conn, caller, NodeId::new(node))
            .await
            .is_err()
        {
            continue;
        }
        let published_at: Option<i64> = sqlx::query_scalar(
            "SELECT published_at FROM incident_publications WHERE incident_id = ? AND page_id = ?",
        )
        .bind(incident.get())
        .bind(page)
        .fetch_optional(&mut *conn)
        .await?;
        let scheduled_at: Option<i64> = sqlx::query_scalar(
            "SELECT due_at FROM incident_auto_publications WHERE incident_id = ? AND page_id = ?",
        )
        .bind(incident.get())
        .bind(page)
        .fetch_optional(&mut *conn)
        .await?;
        pages.push(IncidentPublication {
            page: PageId::new(page),
            slug,
            title,
            published_at,
            scheduled_at,
        });
    }
    Ok(IncidentPublications { pages })
}

/// Publishes auto-incidents whose grace ran out, until `stop`. New auto-incidents wake it
/// through their `incidents:<id>` events.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let (mut wake, _) = state.hub.subscribe();
    while !*stop.borrow() {
        let now = state.clock.now_ms();
        let next = match tick(&state, now).await {
            Ok(next) => next,
            Err(error) => {
                tracing::error!(%error, "publishing auto-incidents failed; retrying");
                Some(now + 5_000)
            }
        };
        let until = next.map_or(now + IDLE_MS, |next| next.min(now + IDLE_MS));
        loop {
            tokio::select! {
                woken = wake.recv() => match woken {
                    Ok(topic) if topic.starts_with("incidents:") => break,
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                },
                _ = state.clock.sleep_until(until) => break,
                _ = stop.changed() => break,
            }
        }
    }
}

/// Publishes what is due and returns when the next one is.
async fn tick(state: &AppState, now: i64) -> anyhow::Result<Option<i64>> {
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let due: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT a.incident_id, a.page_id FROM incident_auto_publications a
         JOIN incidents i ON i.id = a.incident_id
         WHERE a.due_at <= ? AND i.resolved_at IS NULL",
    )
    .bind(now)
    .fetch_all(&mut *tx)
    .await?;
    let mut topics = Vec::new();
    for (incident, page) in &due {
        insert(&mut tx, IncidentId::new(*incident), PageId::new(*page), now).await?;
        let topic = Topic::page(PageId::new(*page));
        crate::events::record(&mut tx, &topic, "page.incident_changed", &json!({}), now).await?;
        topics.push(topic);
    }
    sqlx::query("DELETE FROM incident_auto_publications WHERE due_at <= ?")
        .bind(now)
        .execute(&mut *tx)
        .await?;
    let next: Option<i64> =
        sqlx::query_scalar("SELECT MIN(due_at) FROM incident_auto_publications")
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    for topic in &topics {
        state.hub.publish(topic);
    }
    Ok(next)
}
