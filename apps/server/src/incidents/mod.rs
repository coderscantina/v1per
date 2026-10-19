//! Incidents, static components and maintenance (milestone 08, decision 0004). Owns
//! `incidents.*`, `components.*` and `maintenance.*`, and the auto-incidents the evaluator opens
//! and resolves.
//!
//! Impact follows the monitoring contract: the latest update that names a target sets its
//! impact, and a resolved incident has every target operational. A monitor's down transition
//! opens an auto-incident unless one is open for it; recovery resolves it unless a person has
//! changed it since. Events go to the `incidents:<id>`, `components:<id>` and
//! `maintenance:<id>` topics of the owning node and every ancestor, so subtree lists follow
//! them.

mod components;
pub mod maintenance;

use std::collections::HashMap;

use serde_json::{Value, json};
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{ComponentId, IncidentId, IncidentUpdateId, MonitorId, NodeId, UserId};
use v1per_protocol::operations::incidents::{
    AffectedTarget, Impact, Incident, IncidentArgs, IncidentDeleted, IncidentDetail,
    IncidentOrigin, IncidentPage, IncidentPublications, IncidentPublishArgs, IncidentResult,
    IncidentState, IncidentStatusFilter, IncidentUpdate, IncidentsAddUpdateArgs,
    IncidentsCreateArgs, IncidentsHandlers, IncidentsListArgs, IncidentsUpdateArgs, Target,
    TargetImpact,
};
use v1per_protocol::registry::Snapshot;
use v1per_storage::Status;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::identity::Caller;
use crate::nodes::scope::path_cte;
use crate::pages::{self, Change};
use crate::state::AppState;

const TITLE_MAX: usize = 200;
const MESSAGE_MAX: usize = 10_000;
const PAGE_DEFAULT: u32 = 25;
const PAGE_MAX: u32 = 100;

/// Trimmed `value`, `required` and at most `max` characters, or `invalid_arguments` on `field`.
pub(crate) fn text(
    field: &str,
    value: &str,
    required: bool,
    max: usize,
) -> Result<String, ApiError> {
    let value = value.trim();
    if required && value.is_empty() {
        return Err(FieldErrors::one(field, FieldErrorCode::Required, None));
    }
    if value.chars().count() > max {
        return Err(FieldErrors::one(
            field,
            FieldErrorCode::TooLong,
            Some(max as u32),
        ));
    }
    Ok(value.to_owned())
}

/// Whether `node` is `ancestor` or below it.
pub(crate) async fn subtree_contains(
    conn: &mut SqliteConnection,
    ancestor: NodeId,
    node: NodeId,
) -> sqlx::Result<bool> {
    let found: Option<i64> =
        sqlx::query_scalar(concat!(path_cte!(), "SELECT 1 FROM path WHERE id = ?2"))
            .bind(node.get())
            .bind(ancestor.get())
            .fetch_optional(conn)
            .await?;
    Ok(found.is_some())
}

/// `node` and its ancestors, root first.
async fn path(conn: &mut SqliteConnection, node: NodeId) -> sqlx::Result<Vec<NodeId>> {
    let ids: Vec<i64> = sqlx::query_scalar(concat!(
        path_cte!(),
        "SELECT id FROM path ORDER BY step DESC"
    ))
    .bind(node.get())
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(NodeId::new).collect())
}

/// Records an event outside a command on `topic` of `node` and every ancestor; returns the
/// topics to wake after commit.
pub(crate) async fn record_tree(
    conn: &mut SqliteConnection,
    topic: fn(NodeId) -> Topic,
    node: NodeId,
    kind: &str,
    payload: &Value,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let mut topics = Vec::new();
    for node in path(conn, node).await? {
        let topic = topic(node);
        crate::events::record(conn, &topic, kind, payload, now).await?;
        topics.push(topic);
    }
    Ok(topics)
}

/// Records an event in a command on `topic` of `node` and every ancestor. The node's own
/// topic goes last, so its sequence is the command's revision.
pub(crate) async fn record_tree_ctx(
    ctx: &mut CommandCtx,
    topic: fn(NodeId) -> Topic,
    node: NodeId,
    kind: &str,
    payload: Value,
) -> Result<(), ApiError> {
    for node in path(ctx.conn(), node).await? {
        ctx.record(&topic(node), kind, payload.clone()).await?;
    }
    Ok(())
}

fn enum_text<T: serde::Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => unreachable!("unit enums serialize as strings"),
    }
}

fn enum_from<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, ApiError> {
    serde_json::from_value(Value::String(text.to_owned())).map_err(ApiError::internal)
}

/// The person behind a caller, for timelines: an API key writes as its owner.
fn author(caller: &Caller) -> Option<UserId> {
    match caller {
        Caller::Person { user, .. } => Some(user.clone()),
        Caller::Key(key) => Some(key.owner.clone()),
        Caller::Anonymous { .. } => None,
    }
}

fn columns(target: Target) -> (Option<i64>, Option<i64>) {
    match target {
        Target::Monitor { monitor } => (Some(monitor.get()), None),
        Target::Component { component } => (None, Some(component.get())),
    }
}

fn target_of(monitor: Option<i64>, component: Option<i64>) -> Result<Target, ApiError> {
    match (monitor, component) {
        (Some(monitor), None) => Ok(Target::Monitor {
            monitor: MonitorId::new(monitor),
        }),
        (None, Some(component)) => Ok(Target::Component {
            component: ComponentId::new(component),
        }),
        _ => Err(ApiError::internal(
            "an incident target names neither or both",
        )),
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    title: String,
    state: String,
    auto: bool,
    auto_monitor_id: Option<i64>,
    edited: bool,
    started_at: i64,
    resolved_at: Option<i64>,
    revision: i64,
    updated_at: i64,
}

macro_rules! select_row {
    () => {
        "SELECT id, node_id, title, state, auto, auto_monitor_id, edited, started_at,
                resolved_at, revision, updated_at
         FROM incidents"
    };
}

impl Row {
    fn incident(&self) -> IncidentId {
        IncidentId::new(self.id)
    }

    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }
}

async fn load(conn: &mut SqliteConnection, incident: IncidentId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE id = ?"))
        .bind(incident.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// The affected targets in the order they were added, each with its current impact.
async fn impacts(
    conn: &mut SqliteConnection,
    row: &Row,
) -> Result<Vec<(Target, Option<Impact>)>, ApiError> {
    let targets: Vec<(Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT monitor_id, component_id FROM incident_targets WHERE incident_id = ? ORDER BY rowid",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let named: Vec<(Option<i64>, Option<i64>, String)> = sqlx::query_as(
        "SELECT i.monitor_id, i.component_id, i.impact
         FROM incident_update_impacts i JOIN incident_updates u ON u.id = i.update_id
         WHERE u.incident_id = ? ORDER BY u.id",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let mut latest: HashMap<Target, Impact> = HashMap::new();
    for (monitor, component, impact) in named {
        latest.insert(target_of(monitor, component)?, enum_from(&impact)?);
    }
    let resolved = row.resolved_at.is_some();
    targets
        .into_iter()
        .map(|(monitor, component)| {
            let target = target_of(monitor, component)?;
            let impact = if resolved {
                Some(Impact::Operational)
            } else {
                latest.get(&target).copied()
            };
            Ok((target, impact))
        })
        .collect()
}

async fn summary(conn: &mut SqliteConnection, row: Row) -> Result<Incident, ApiError> {
    let affected = impacts(conn, &row).await?;
    Ok(Incident {
        id: row.incident(),
        node: row.node(),
        state: enum_from(&row.state)?,
        impact: affected.iter().filter_map(|(_, impact)| *impact).max(),
        origin: if row.auto {
            IncidentOrigin::Auto
        } else {
            IncidentOrigin::Manual
        },
        auto_monitor: row.auto_monitor_id.map(MonitorId::new),
        edited: row.edited,
        affected: affected.len() as u32,
        title: row.title,
        started_at: row.started_at,
        resolved_at: row.resolved_at,
        updated_at: row.updated_at,
        revision: row.revision,
    })
}

/// Every target must be a monitor or component on `node` or below, each named once.
pub(crate) async fn check_targets(
    conn: &mut SqliteConnection,
    node: NodeId,
    field: &str,
    targets: &[Target],
) -> Result<(), ApiError> {
    let invalid = || FieldErrors::one(field, FieldErrorCode::InvalidValue, None);
    let mut seen = Vec::new();
    for &target in targets {
        if seen.contains(&target) {
            return Err(invalid());
        }
        seen.push(target);
        let owner: Option<i64> = match target {
            Target::Monitor { monitor } => {
                sqlx::query_scalar("SELECT node_id FROM monitors WHERE id = ?")
                    .bind(monitor.get())
                    .fetch_optional(&mut *conn)
                    .await?
            }
            Target::Component { component } => {
                sqlx::query_scalar("SELECT node_id FROM components WHERE id = ?")
                    .bind(component.get())
                    .fetch_optional(&mut *conn)
                    .await?
            }
        };
        let inside = match owner {
            Some(owner) => subtree_contains(conn, node, NodeId::new(owner)).await?,
            None => false,
        };
        if !inside {
            return Err(invalid());
        }
    }
    Ok(())
}

fn targets(impacts: &[TargetImpact]) -> Vec<Target> {
    impacts.iter().map(|entry| entry.target).collect()
}

async fn add_targets(
    conn: &mut SqliteConnection,
    incident: i64,
    targets: &[Target],
) -> sqlx::Result<()> {
    for &target in targets {
        let (monitor, component) = columns(target);
        sqlx::query(
            "INSERT OR IGNORE INTO incident_targets (incident_id, monitor_id, component_id)
             VALUES (?, ?, ?)",
        )
        .bind(incident)
        .bind(monitor)
        .bind(component)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Appends an update with its impacts.
async fn add_update(
    conn: &mut SqliteConnection,
    incident: i64,
    state: IncidentState,
    message: &str,
    impacts: &[TargetImpact],
    author: Option<&UserId>,
    now: i64,
) -> sqlx::Result<()> {
    let update: i64 = sqlx::query_scalar(
        "INSERT INTO incident_updates (incident_id, state, message, author_id, created_at)
         VALUES (?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(incident)
    .bind(enum_text(state))
    .bind(message)
    .bind(author.map(UserId::as_str))
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    for entry in impacts {
        let (monitor, component) = columns(entry.target);
        sqlx::query(
            "INSERT INTO incident_update_impacts (update_id, monitor_id, component_id, impact)
             VALUES (?, ?, ?, ?)",
        )
        .bind(update)
        .bind(monitor)
        .bind(component)
        .bind(enum_text(entry.impact))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Opens or resolves the monitor's auto-incident on an important transition. Returns the
/// topics to wake.
pub(crate) async fn on_transition(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    status: Status,
    message: &str,
    at: i64,
) -> sqlx::Result<Vec<Topic>> {
    let open: Option<(i64, i64, bool)> = sqlx::query_as(
        "SELECT id, node_id, edited FROM incidents
         WHERE auto_monitor_id = ? AND resolved_at IS NULL",
    )
    .bind(monitor.get())
    .fetch_optional(&mut *conn)
    .await?;
    let pages_touched;
    let (id, node, kind) = match (status, open) {
        (Status::Down, None) => {
            let (node, name): (i64, String) =
                sqlx::query_as("SELECT node_id, name FROM monitors WHERE id = ?")
                    .bind(monitor.get())
                    .fetch_one(&mut *conn)
                    .await?;
            let id: i64 = sqlx::query_scalar(
                "INSERT INTO incidents (node_id, title, title_kind, state, auto, auto_monitor_id,
                                        started_at, revision, updated_at)
                 VALUES (?, ?, 'monitor_down', 'investigating', 1, ?, ?, 1, ?) RETURNING id",
            )
            .bind(node)
            .bind(format!("{name} is down"))
            .bind(monitor.get())
            .bind(at)
            .bind(at)
            .fetch_one(&mut *conn)
            .await?;
            let target = Target::Monitor { monitor };
            add_targets(conn, id, &[target]).await?;
            let impact = TargetImpact {
                target,
                impact: Impact::MajorOutage,
            };
            add_update(
                conn,
                id,
                IncidentState::Investigating,
                message,
                &[impact],
                None,
                at,
            )
            .await?;
            let incident = IncidentId::new(id);
            pages_touched = pages::publish::auto_opened(conn, incident, monitor, at).await?;
            (id, node, "incident.created")
        }
        (Status::Up, Some((id, node, false))) => {
            add_update(conn, id, IncidentState::Resolved, message, &[], None, at).await?;
            sqlx::query(
                "UPDATE incidents SET state = 'resolved', resolved_at = ?,
                                      revision = revision + 1, updated_at = ?
                 WHERE id = ?",
            )
            .bind(at)
            .bind(at)
            .bind(id)
            .execute(&mut *conn)
            .await?;
            pages_touched = pages::publish::auto_resolved(conn, IncidentId::new(id), at).await?;
            (id, node, "incident.updated")
        }
        _ => return Ok(Vec::new()),
    };
    let payload = json!({ "incident": IncidentId::new(id) });
    let mut topics = record_tree(
        conn,
        Topic::incidents,
        NodeId::new(node),
        kind,
        &payload,
        at,
    )
    .await?;
    topics.extend(pages_touched);
    Ok(topics)
}

async fn answer(ctx: &mut CommandCtx, incident: IncidentId) -> Result<IncidentResult, ApiError> {
    let row = load(ctx.conn(), incident).await?;
    Ok(IncidentResult {
        incident: summary(ctx.conn(), row).await?,
    })
}

async fn record(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    incident: IncidentId,
) -> Result<(), ApiError> {
    pages::touch_in(ctx, Change::Incident(incident)).await?;
    record_tree_ctx(
        ctx,
        Topic::incidents,
        node,
        kind,
        json!({ "incident": incident }),
    )
    .await
}

impl IncidentsHandlers for AppState {
    async fn incidents_create(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentsCreateArgs,
    ) -> Result<IncidentResult, ApiError> {
        ctx.node_scope(args.node, Capability::IncidentsManage)
            .await?;
        let title = text("title", &args.title, true, TITLE_MAX)?;
        let message = text("message", &args.message, false, MESSAGE_MAX)?;
        check_targets(ctx.conn(), args.node, "impacts", &targets(&args.impacts)).await?;
        let now = ctx.now();
        let resolved = (args.state == IncidentState::Resolved).then_some(now);
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO incidents (node_id, title, state, auto, started_at, resolved_at,
                                    revision, updated_at)
             VALUES (?, ?, ?, 0, ?, ?, 1, ?) RETURNING id",
        )
        .bind(args.node.get())
        .bind(&title)
        .bind(enum_text(args.state))
        .bind(now)
        .bind(resolved)
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
        add_targets(ctx.conn(), id, &targets(&args.impacts)).await?;
        let author = author(ctx.caller());
        add_update(
            ctx.conn(),
            id,
            args.state,
            &message,
            &args.impacts,
            author.as_ref(),
            now,
        )
        .await?;
        let incident = IncidentId::new(id);
        pages::publish::publish_defaults(ctx, incident, &targets(&args.impacts)).await?;
        record(ctx, args.node, "incident.created", incident).await?;
        answer(ctx, incident).await
    }

    async fn incidents_add_update(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentsAddUpdateArgs,
    ) -> Result<IncidentResult, ApiError> {
        let row = load(ctx.conn(), args.incident).await?;
        ctx.node_scope(row.node(), Capability::IncidentsManage)
            .await?;
        let message = text("message", &args.message, false, MESSAGE_MAX)?;
        check_targets(ctx.conn(), row.node(), "impacts", &targets(&args.impacts)).await?;
        let resolving = args.state == IncidentState::Resolved;
        if !resolving
            && row.resolved_at.is_some()
            && let Some(monitor) = row.auto_monitor_id
        {
            let newer: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM incidents WHERE auto_monitor_id = ? AND resolved_at IS NULL",
            )
            .bind(monitor)
            .fetch_optional(ctx.conn())
            .await?;
            if newer.is_some() {
                return Err(FieldErrors::one(
                    "state",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        }
        let now = ctx.now();
        add_targets(ctx.conn(), row.id, &targets(&args.impacts)).await?;
        let author = author(ctx.caller());
        add_update(
            ctx.conn(),
            row.id,
            args.state,
            &message,
            &args.impacts,
            author.as_ref(),
            now,
        )
        .await?;
        sqlx::query(
            "UPDATE incidents
             SET state = ?, resolved_at = CASE WHEN ? THEN coalesce(resolved_at, ?) END,
                 edited = auto, revision = revision + 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(enum_text(args.state))
        .bind(resolving)
        .bind(now)
        .bind(now)
        .bind(row.id)
        .execute(ctx.conn())
        .await?;
        record(ctx, row.node(), "incident.updated", args.incident).await?;
        answer(ctx, args.incident).await
    }

    async fn incidents_update(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentsUpdateArgs,
    ) -> Result<IncidentResult, ApiError> {
        let row = load(ctx.conn(), args.incident).await?;
        ctx.node_scope(row.node(), Capability::IncidentsManage)
            .await?;
        if let Some(expected) = args.expected_revision
            && expected != row.revision
        {
            return Err(ApiError::stale_revision(row.revision));
        }
        let title = text("title", &args.title, true, TITLE_MAX)?;
        check_targets(ctx.conn(), row.node(), "affected", &args.affected).await?;
        let current: Vec<(Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT monitor_id, component_id FROM incident_targets WHERE incident_id = ?",
        )
        .bind(row.id)
        .fetch_all(ctx.conn())
        .await?;
        for (monitor, component) in current {
            if !args.affected.contains(&target_of(monitor, component)?) {
                sqlx::query(
                    "DELETE FROM incident_targets
                     WHERE incident_id = ? AND monitor_id IS ? AND component_id IS ?",
                )
                .bind(row.id)
                .bind(monitor)
                .bind(component)
                .execute(ctx.conn())
                .await?;
            }
        }
        add_targets(ctx.conn(), row.id, &args.affected).await?;
        sqlx::query(
            "UPDATE incidents
             SET title_kind = CASE WHEN title = ?1 THEN title_kind END, title = ?1,
                 edited = auto, revision = revision + 1, updated_at = ?2
             WHERE id = ?3",
        )
        .bind(&title)
        .bind(ctx.now())
        .bind(row.id)
        .execute(ctx.conn())
        .await?;
        record(ctx, row.node(), "incident.updated", args.incident).await?;
        answer(ctx, args.incident).await
    }

    async fn incidents_delete(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentArgs,
    ) -> Result<IncidentDeleted, ApiError> {
        let row = load(ctx.conn(), args.incident).await?;
        ctx.node_scope(row.node(), Capability::IncidentsManage)
            .await?;
        // Publications cascade, so the pages are told first.
        pages::touch_in(ctx, Change::Incident(args.incident)).await?;
        sqlx::query("DELETE FROM incidents WHERE id = ?")
            .bind(row.id)
            .execute(ctx.conn())
            .await?;
        record(ctx, row.node(), "incident.deleted", args.incident).await?;
        Ok(IncidentDeleted {
            incident: args.incident,
        })
    }

    async fn incidents_publish(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentPublishArgs,
    ) -> Result<IncidentPublications, ApiError> {
        pages::publish::publish(ctx, args.incident, args.page, true).await
    }

    async fn incidents_unpublish(
        &self,
        ctx: &mut CommandCtx,
        args: IncidentPublishArgs,
    ) -> Result<IncidentPublications, ApiError> {
        pages::publish::publish(ctx, args.incident, args.page, false).await
    }

    async fn incidents_publications(
        &self,
        ctx: &mut QueryCtx,
        args: IncidentArgs,
    ) -> Result<Snapshot<IncidentPublications>, ApiError> {
        let row = load(ctx.conn(), args.incident).await?;
        ctx.node_scope(row.node(), Capability::NodeRead).await?;
        let pages = pages::publish::publications(ctx, args.incident).await?;
        Ok(Snapshot {
            topic: Topic::incidents(row.node()),
            data: pages,
        })
    }

    async fn incidents_list(
        &self,
        ctx: &mut QueryCtx,
        args: IncidentsListArgs,
    ) -> Result<Snapshot<IncidentPage>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        let before = match &args.cursor {
            None => i64::MAX,
            Some(cursor) => cursor
                .parse::<i64>()
                .map_err(|_| FieldErrors::one("cursor", FieldErrorCode::InvalidFormat, None))?,
        };
        let limit = args.limit.unwrap_or(PAGE_DEFAULT).clamp(1, PAGE_MAX);
        let rows: Vec<Row> = sqlx::query_as(concat!(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id WHERE ?2
             ) ",
            select_row!(),
            " WHERE node_id IN (SELECT id FROM below)
                AND (?3 IS NULL OR (?3 = 'open') = (resolved_at IS NULL))
                AND (?4 IS NULL OR (?4 = 'auto') = auto)
                AND (?5 IS NULL OR EXISTS (SELECT 1 FROM incident_targets t
                                           WHERE t.incident_id = incidents.id
                                             AND t.monitor_id = ?5))
                AND id < ?6
              ORDER BY id DESC LIMIT ?7"
        ))
        .bind(args.node.get())
        .bind(args.subtree)
        .bind(args.status.map(|status| match status {
            IncidentStatusFilter::Open => "open",
            IncidentStatusFilter::Resolved => "resolved",
        }))
        .bind(args.origin.map(|origin| match origin {
            IncidentOrigin::Auto => "auto",
            IncidentOrigin::Manual => "manual",
        }))
        .bind(args.monitor.map(MonitorId::get))
        .bind(before)
        .bind(i64::from(limit) + 1)
        .fetch_all(ctx.conn())
        .await?;
        let more = rows.len() > limit as usize;
        let mut incidents = Vec::new();
        for row in rows.into_iter().take(limit as usize) {
            incidents.push(summary(ctx.conn(), row).await?);
        }
        let next_cursor = more
            .then(|| incidents.last().map(|last: &Incident| last.id.to_string()))
            .flatten();
        Ok(Snapshot {
            topic: Topic::incidents(args.node),
            data: IncidentPage {
                incidents,
                next_cursor,
            },
        })
    }

    async fn incidents_get(
        &self,
        ctx: &mut QueryCtx,
        args: IncidentArgs,
    ) -> Result<Snapshot<IncidentDetail>, ApiError> {
        let row = load(ctx.conn(), args.incident).await?;
        let node = row.node();
        ctx.node_scope(node, Capability::NodeRead).await?;
        let mut affected = Vec::new();
        for (target, impact) in impacts(ctx.conn(), &row).await? {
            let name: String = match target {
                Target::Monitor { monitor } => {
                    sqlx::query_scalar("SELECT name FROM monitors WHERE id = ?")
                        .bind(monitor.get())
                        .fetch_one(ctx.conn())
                        .await?
                }
                Target::Component { component } => {
                    sqlx::query_scalar("SELECT name FROM components WHERE id = ?")
                        .bind(component.get())
                        .fetch_one(ctx.conn())
                        .await?
                }
            };
            affected.push(AffectedTarget {
                target,
                name,
                impact,
            });
        }
        let rows: Vec<(i64, String, String, Option<String>, i64)> = sqlx::query_as(
            "SELECT u.id, u.state, u.message, p.name, u.created_at
             FROM incident_updates u LEFT JOIN users p ON p.id = u.author_id
             WHERE u.incident_id = ? ORDER BY u.id",
        )
        .bind(row.id)
        .fetch_all(ctx.conn())
        .await?;
        let named: Vec<(i64, Option<i64>, Option<i64>, String)> = sqlx::query_as(
            "SELECT i.update_id, i.monitor_id, i.component_id, i.impact
             FROM incident_update_impacts i JOIN incident_updates u ON u.id = i.update_id
             WHERE u.incident_id = ? ORDER BY i.rowid",
        )
        .bind(row.id)
        .fetch_all(ctx.conn())
        .await?;
        let mut updates = Vec::with_capacity(rows.len());
        for (id, state, message, author, created_at) in rows {
            let impacts = named
                .iter()
                .filter(|(update, ..)| *update == id)
                .map(|(_, monitor, component, impact)| {
                    Ok(TargetImpact {
                        target: target_of(*monitor, *component)?,
                        impact: enum_from(impact)?,
                    })
                })
                .collect::<Result<_, ApiError>>()?;
            updates.push(IncidentUpdate {
                id: IncidentUpdateId::new(id),
                state: enum_from(&state)?,
                message,
                impacts,
                author,
                created_at,
            });
        }
        Ok(Snapshot {
            topic: Topic::incidents(node),
            data: IncidentDetail {
                incident: summary(ctx.conn(), row).await?,
                affected,
                updates,
            },
        })
    }
}
