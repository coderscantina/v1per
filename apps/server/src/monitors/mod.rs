//! Monitors (milestone 05): definitions on nodes, their evaluation and history. Owns
//! `monitors.*` and `ALL /api/push/{token}`, and tells probes what to run.
//!
//! Topics: changes and status changes are recorded on the node's `monitors:<id>` topic as
//! `monitor.created`, `monitor.updated`, `monitor.paused`, `monitor.resumed`,
//! `monitor.deleted` and `monitor.status_changed`, each with the monitor's ID, `monitor.moved`
//! with the monitor and its node on both the old and the new node's topics, and
//! `monitor.probe_changed` with the monitor, the probe and its status when one probe's view of
//! a monitor changes. Each also goes to the `monitor-tree:<id>` topic of the node and every
//! ancestor, which subtree lists follow.

mod evaluator;
mod history;
mod push;
pub(crate) mod rules;
pub(crate) mod settings;

pub(crate) use evaluator::live_probes;
pub use evaluator::{BatchRefusal, Evaluator, Queue, run as run_evaluator};
pub(crate) use push::push;

use std::collections::HashSet;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{MonitorId, NodeId, ProbeId};
use v1per_protocol::operations::monitors::{
    DayBarList, DayBarsArgs, HeartbeatPage, HeartbeatStatus, HeartbeatsArgs, LatencyArgs,
    LatencySeries, ManualStatus, Monitor, MonitorArgs, MonitorDeleted, MonitorDetail,
    MonitorHourlyList, MonitorList, MonitorResult, MonitorSettings, MonitorState, MonitorSummary,
    MonitorType, MonitorUptime, MonitorsCreateArgs, MonitorsHandlers, MonitorsListArgs,
    MonitorsMoveArgs, MonitorsSetManualStatusArgs, MonitorsUpdateArgs, ProbeState, ScopeTrend,
    UptimeArgs,
};
use v1per_protocol::operations::nodes::NodeArgs;
use v1per_protocol::registry::Snapshot;
use v1per_storage::Status;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::events;
use crate::identity::secrets::SecretKey;
use crate::incidents::maintenance;
use crate::nodes::scope::{NodeScope, path_cte};
use crate::notifications;
use crate::pages::{self, Change};
use crate::probes::{self, wire};
use crate::state::AppState;

/// `SELECT <every Row column> FROM monitors`, for `concat!` with a condition.
macro_rules! select_row {
    () => {
        "SELECT id, node_id, position, group_id, name, description, type, settings, interval,
                retry_interval, max_retries, resend_interval, timeout, upside_down, quorum,
                active, tags, manual_status, revision, created_at, updated_at
         FROM monitors"
    };
}

/// What a probe runs for one active monitor.
pub enum Assignment {
    /// A check, run alike by the built-in probe and agents.
    Check(Box<wire::Assignment>),
    /// A round the server evaluates itself, on the built-in probe only.
    Round(Round),
}

/// A push deadline, a group or manual monitor's round, or a maintenance beat.
pub struct Round {
    pub monitor: MonitorId,
    pub revision: i64,
    pub interval_ms: i64,
    pub retry_interval_ms: i64,
    pub updated_at: i64,
    /// A push monitor: the first round waits one interval, as in Kuma.
    pub push: bool,
    /// Under maintenance: the first maintenance beat is due at once.
    pub maintenance: bool,
}

impl Assignment {
    pub fn revision(&self) -> i64 {
        match self {
            Assignment::Check(check) => check.revision,
            Assignment::Round(round) => round.revision,
        }
    }
}

/// Moves whenever any monitor row, monitor's probes or probe revocation changes, node moves
/// and deletions included.
pub async fn assignment_version(conn: &mut SqliteConnection) -> sqlx::Result<i64> {
    sqlx::query_scalar("SELECT version FROM assignments WHERE id = 1")
        .fetch_one(conn)
        .await
}

/// An active monitor a probe runs. Its task is rebuilt when either field changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    pub monitor: MonitorId,
    pub revision: i64,
    pub maintenance: bool,
}

/// The active monitors `probe` runs: those assigned to it and not under maintenance, and with
/// `rounds` every push, group and manual monitor and every monitor under maintenance.
pub async fn active(
    conn: &mut SqliteConnection,
    probe: ProbeId,
    rounds: bool,
) -> sqlx::Result<Vec<Active>> {
    let rows: Vec<(i64, i64, bool)> = sqlx::query_as(
        "SELECT m.id, m.revision, s.monitor_id IS NOT NULL FROM monitors m
         LEFT JOIN maintenance_state s ON s.monitor_id = m.id
         WHERE m.active = 1
           AND ((s.monitor_id IS NULL AND EXISTS (SELECT 1 FROM monitor_probes mp
                                                  WHERE mp.monitor_id = m.id AND mp.probe_id = ?1))
                OR (?2 AND (s.monitor_id IS NOT NULL OR m.type IN ('push', 'group', 'manual'))))
         ORDER BY m.id",
    )
    .bind(probe.get())
    .bind(rounds)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, revision, maintenance)| Active {
            monitor: MonitorId::new(id),
            revision,
            maintenance,
        })
        .collect())
}

/// What to run for `monitor`, or `None` when it is paused, gone or cannot run: an
/// unsupported type, or settings whose secrets do not open.
pub async fn assignment(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    monitor: MonitorId,
) -> anyhow::Result<Option<Assignment>> {
    let row: Option<Row> = sqlx::query_as(concat!(select_row!(), " WHERE id = ? AND active = 1"))
        .bind(monitor.get())
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let settings: MonitorSettings = serde_json::from_str(&row.settings)?;
    let maintenance: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM maintenance_state WHERE monitor_id = ?")
            .bind(monitor.get())
            .fetch_optional(&mut *conn)
            .await?;
    let round = |push, maintenance| {
        Assignment::Round(Round {
            monitor,
            revision: row.revision,
            interval_ms: row.interval * 1000,
            retry_interval_ms: row.retry_interval * 1000,
            updated_at: row.updated_at,
            push,
            maintenance,
        })
    };
    let assignment = match &settings {
        MonitorSettings::Unsupported(_) => return Ok(None),
        _ if maintenance.is_some() => round(false, true),
        MonitorSettings::Push(_) => round(true, false),
        MonitorSettings::Group | MonitorSettings::Manual => round(false, false),
        _ => {
            let secrets = settings::open(conn, key, monitor).await?;
            let Some(definition) =
                settings::check_definition(&settings, &secrets, row.timeout, row.upside_down)
            else {
                tracing::error!(%monitor, "a monitor's settings do not build a check");
                return Ok(None);
            };
            Assignment::Check(Box::new(wire::Assignment {
                monitor,
                revision: row.revision,
                interval_ms: row.interval * 1000,
                retry_interval_ms: row.retry_interval * 1000,
                max_retries: row.max_retries as u32,
                down_on_query_mismatch: matches!(
                    &settings,
                    MonitorSettings::JsonQuery(query) if query.retry_only_on_status_code_failure
                ),
                updated_at: row.updated_at,
                definition,
            }))
        }
    };
    Ok(Some(assignment))
}

/// Whether a probe runs monitors of these settings; the server runs push, group and manual
/// monitors itself.
pub(crate) fn runs_on_probes(settings: &MonitorSettings) -> bool {
    !matches!(
        settings,
        MonitorSettings::Push(_)
            | MonitorSettings::Group
            | MonitorSettings::Manual
            | MonitorSettings::Unsupported(_)
    )
}

/// A monitor row as stored.
#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    position: i64,
    group_id: Option<i64>,
    name: String,
    description: String,
    #[sqlx(rename = "type")]
    kind: String,
    settings: String,
    interval: i64,
    retry_interval: i64,
    max_retries: i64,
    resend_interval: i64,
    timeout: f64,
    upside_down: bool,
    quorum: String,
    active: bool,
    tags: String,
    manual_status: Option<i64>,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

/// One line of `monitors.list`.
#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: i64,
    node_id: i64,
    position: i64,
    name: String,
    #[sqlx(rename = "type")]
    kind: String,
    active: bool,
    tags: String,
    group_id: Option<i64>,
    status: Option<i64>,
    latency_ms: Option<i64>,
    beat_at: Option<i64>,
}

/// One probe's state of a monitor, with the probe's name.
#[derive(sqlx::FromRow)]
struct ProbeRow {
    id: i64,
    name: String,
    status: i64,
    message: String,
    latency_ms: Option<i64>,
    retries: i64,
    checked_at: i64,
    received_at: i64,
}

impl Row {
    fn monitor(&self) -> MonitorId {
        MonitorId::new(self.id)
    }

    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }
}

/// The monitor, before any scope check. Callers resolve the scope on its node next, so an
/// unknown monitor and one on a hidden node answer the same `not_found`.
async fn load(conn: &mut SqliteConnection, monitor: MonitorId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE id = ?"))
        .bind(monitor.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// The monitor as the caller sees it: the push token only for those who manage it.
async fn present(
    conn: &mut SqliteConnection,
    key: &SecretKey,
    row: Row,
    manages: bool,
) -> Result<Monitor, ApiError> {
    let mut settings: MonitorSettings =
        serde_json::from_str(&row.settings).map_err(ApiError::internal)?;
    if manages && matches!(settings, MonitorSettings::Push(_)) {
        let secrets = settings::open(conn, key, row.monitor()).await?;
        settings::reveal_push_token(&mut settings, &secrets);
    }
    let probes = probes::of_monitor(conn, row.monitor()).await?;
    let channels = notifications::of_monitor(conn, row.monitor()).await?;
    Ok(Monitor {
        id: row.monitor(),
        node: row.node(),
        position: row.position as u32,
        name: row.name,
        description: row.description,
        settings,
        interval: row.interval as u32,
        retry_interval: row.retry_interval as u32,
        max_retries: row.max_retries as u32,
        resend_interval: row.resend_interval as u32,
        timeout: row.timeout,
        upside_down: row.upside_down,
        quorum: from_text(&row.quorum).map_err(ApiError::internal)?,
        tags: serde_json::from_str(&row.tags).map_err(ApiError::internal)?,
        group: row.group_id.map(MonitorId::new),
        probes,
        channels,
        active: row.active,
        revision: row.revision,
        manual_status: row
            .manual_status
            .and_then(status_from_code)
            .map(|status| match status {
                Status::Up => ManualStatus::Up,
                Status::Down => ManualStatus::Down,
                _ => ManualStatus::Pending,
            }),
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Reads a unit enum from its serde name, as stored in text columns.
fn from_text<T: DeserializeOwned>(text: &str) -> serde_json::Result<T> {
    serde_json::from_value(Value::String(text.to_owned()))
}

fn to_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(text)) => text,
        _ => unreachable!("unit enums serialize as strings"),
    }
}

fn monitor_type(settings: &MonitorSettings) -> MonitorType {
    match settings {
        MonitorSettings::Http(_) => MonitorType::Http,
        MonitorSettings::Keyword(_) => MonitorType::Keyword,
        MonitorSettings::JsonQuery(_) => MonitorType::JsonQuery,
        MonitorSettings::Port(_) => MonitorType::Port,
        MonitorSettings::Ping(_) => MonitorType::Ping,
        MonitorSettings::Dns(_) => MonitorType::Dns,
        MonitorSettings::Push(_) => MonitorType::Push,
        MonitorSettings::Group => MonitorType::Group,
        MonitorSettings::Manual => MonitorType::Manual,
        MonitorSettings::Unsupported(_) => MonitorType::Unsupported,
    }
}

/// The `type` column: Kuma's type name.
pub(crate) fn type_text(settings: &MonitorSettings) -> String {
    to_text(&monitor_type(settings))
}

/// Kuma's status code, as stored.
pub(crate) fn status_code(status: Status) -> i64 {
    i64::from(status as u8)
}

pub(crate) fn status_from_code(code: i64) -> Option<Status> {
    match code {
        0 => Some(Status::Down),
        1 => Some(Status::Up),
        2 => Some(Status::Pending),
        3 => Some(Status::Maintenance),
        _ => None,
    }
}

pub(crate) fn wire_status(status: Status) -> HeartbeatStatus {
    match status {
        Status::Down => HeartbeatStatus::Down,
        Status::Up => HeartbeatStatus::Up,
        Status::Pending => HeartbeatStatus::Pending,
        Status::Maintenance => HeartbeatStatus::Maintenance,
    }
}

pub(crate) fn status_from_wire(status: HeartbeatStatus) -> Status {
    match status {
        HeartbeatStatus::Down => Status::Down,
        HeartbeatStatus::Up => Status::Up,
        HeartbeatStatus::Pending => Status::Pending,
        HeartbeatStatus::Maintenance => Status::Maintenance,
    }
}

fn stored_status(code: i64) -> Result<HeartbeatStatus, ApiError> {
    status_from_code(code)
        .map(wire_status)
        .ok_or_else(|| ApiError::internal(format!("status code {code}")))
}

/// `group` must be a group monitor on the member's node or an ancestor the caller can read,
/// and must not be the member or one of its own members. Anything else is `invalid_value`,
/// whether the group exists or not.
async fn check_group(
    conn: &mut SqliteConnection,
    scope: &NodeScope,
    group: MonitorId,
    member: Option<MonitorId>,
) -> Result<(), ApiError> {
    let invalid = || FieldErrors::one("group", FieldErrorCode::InvalidValue, None);
    let found: Option<(i64, String)> =
        sqlx::query_as("SELECT node_id, type FROM monitors WHERE id = ?")
            .bind(group.get())
            .fetch_optional(&mut *conn)
            .await?;
    let Some((node, kind)) = found else {
        return Err(invalid());
    };
    let node = NodeId::new(node);
    let readable = node == scope.node()
        || scope
            .ancestors()
            .any(|(ancestor, readable)| readable && ancestor == node);
    if !readable || kind != to_text(&MonitorType::Group) {
        return Err(invalid());
    }
    if let Some(member) = member {
        let cycle: Option<i64> = sqlx::query_scalar(
            "WITH RECURSIVE up(id) AS (
                 SELECT ?1
                 UNION
                 SELECT m.group_id FROM monitors m JOIN up ON m.id = up.id
                 WHERE m.group_id IS NOT NULL
             )
             SELECT 1 FROM up WHERE id = ?2",
        )
        .bind(group.get())
        .bind(member.get())
        .fetch_optional(conn)
        .await?;
        if cycle.is_some() {
            return Err(invalid());
        }
    }
    Ok(())
}

/// The topics a monitor event on `node` goes to: the `monitor-tree:<id>` topic of the root
/// down to the node, then the node's `monitors:<id>`, whose sequence a command answers.
async fn event_topics(conn: &mut SqliteConnection, node: NodeId) -> sqlx::Result<Vec<Topic>> {
    let path: Vec<i64> = sqlx::query_scalar(concat!(
        path_cte!(),
        "SELECT id FROM path ORDER BY step DESC"
    ))
    .bind(node.get())
    .fetch_all(&mut *conn)
    .await?;
    let mut topics: Vec<Topic> = path
        .into_iter()
        .map(|id| Topic::monitor_tree(NodeId::new(id)))
        .collect();
    topics.push(Topic::monitors(node));
    Ok(topics)
}

/// Records a monitor event in a command on every topic it goes to.
pub(crate) async fn record_event(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    payload: Value,
) -> Result<(), ApiError> {
    for topic in event_topics(ctx.conn(), node).await? {
        ctx.record(&topic, kind, payload.clone()).await?;
    }
    Ok(())
}

/// Records a monitor event outside a command on every topic it goes to, and returns them to
/// wake after commit.
pub(crate) async fn record_evaluated(
    conn: &mut SqliteConnection,
    node: NodeId,
    kind: &str,
    payload: &Value,
    now: i64,
) -> sqlx::Result<Vec<Topic>> {
    let topics = event_topics(conn, node).await?;
    for topic in &topics {
        events::record(conn, topic, kind, payload, now).await?;
    }
    Ok(topics)
}

async fn record(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    monitor: MonitorId,
) -> Result<(), ApiError> {
    pages::touch_in(ctx, Change::Monitor(monitor)).await?;
    record_event(ctx, node, kind, json!({ "monitor": monitor })).await
}

/// The monitor's row and the caller's scope on its node, with `capability`.
async fn locate(
    ctx: &mut CommandCtx,
    monitor: MonitorId,
    capability: Capability,
) -> Result<(Row, NodeScope), ApiError> {
    let row = load(ctx.conn(), monitor).await?;
    let scope = ctx.node_scope(row.node(), capability).await?;
    Ok((row, scope))
}

/// A command's answer: the monitor as its manager sees it.
async fn answer(
    ctx: &mut CommandCtx,
    key: &SecretKey,
    monitor: MonitorId,
) -> Result<MonitorResult, ApiError> {
    let row = load(ctx.conn(), monitor).await?;
    Ok(MonitorResult {
        monitor: present(ctx.conn(), key, row, true).await?,
    })
}

/// `node.read` on the monitor's node; the topic its history queries belong to.
async fn readable(ctx: &mut QueryCtx, monitor: MonitorId) -> Result<Topic, ApiError> {
    let row = load(ctx.conn(), monitor).await?;
    let scope = ctx.node_scope(row.node(), Capability::NodeRead).await?;
    Ok(Topic::monitors(scope.node()))
}

/// `node.read` on `node`; the monitors on it and below.
async fn monitors_below(ctx: &mut QueryCtx, node: NodeId) -> Result<Vec<MonitorId>, ApiError> {
    ctx.node_scope(node, Capability::NodeRead).await?;
    Ok(on_subtree(ctx.conn(), node).await?)
}

/// The monitors on `node` and below.
pub(crate) async fn on_subtree(
    conn: &mut SqliteConnection,
    node: NodeId,
) -> sqlx::Result<Vec<MonitorId>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE below(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id
         )
         SELECT m.id FROM monitors m WHERE m.node_id IN (SELECT id FROM below)",
    )
    .bind(node.get())
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(MonitorId::new).collect())
}

/// `monitor` first, then the members that move with it off `node`: those on `node` in it or in
/// one of its groups there, in their order.
async fn with_members_on(
    conn: &mut SqliteConnection,
    monitor: MonitorId,
    node: NodeId,
) -> sqlx::Result<Vec<MonitorId>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "WITH RECURSIVE moved(id) AS (
             SELECT ?1
             UNION
             SELECT m.id FROM monitors m JOIN moved ON m.group_id = moved.id
             WHERE m.node_id = ?2
         )
         SELECT m.id FROM moved JOIN monitors m ON m.id = moved.id
         ORDER BY m.id <> ?1, m.position, m.id",
    )
    .bind(monitor.get())
    .bind(node.get())
    .fetch_all(conn)
    .await?;
    Ok(ids.into_iter().map(MonitorId::new).collect())
}

/// Numbers the monitors on `node` from 0 in their order, with `placed` put together at its
/// index (past the end means last).
async fn renumber(
    conn: &mut SqliteConnection,
    node: NodeId,
    placed: Option<(&[MonitorId], u32)>,
) -> sqlx::Result<()> {
    let mut monitors: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM monitors WHERE node_id = ? ORDER BY position, id")
            .bind(node.get())
            .fetch_all(&mut *conn)
            .await?;
    if let Some((placed, index)) = placed {
        monitors.retain(|id| !placed.iter().any(|monitor| monitor.get() == *id));
        let index = (index as usize).min(monitors.len());
        monitors.splice(index..index, placed.iter().map(|monitor| monitor.get()));
    }
    for (position, id) in monitors.into_iter().enumerate() {
        sqlx::query("UPDATE monitors SET position = ?1 WHERE id = ?2 AND position <> ?1")
            .bind(position as i64)
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

impl MonitorsHandlers for AppState {
    async fn monitors_create(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorsCreateArgs,
    ) -> Result<MonitorResult, ApiError> {
        let scope = ctx
            .node_scope(args.node, Capability::MonitorsManage)
            .await?;
        let group = args.monitor.group;
        let channels = args.monitor.channels.clone();
        let valid = settings::validate(args.monitor, &HashSet::new())?;
        if let Some(group) = group {
            check_group(ctx.conn(), &scope, group, None).await?;
        }
        let now = ctx.now();
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO monitors (node_id, position, group_id, name, description, type, settings,
                                   interval, retry_interval, max_retries, resend_interval,
                                   timeout, upside_down, quorum, active, tags, revision,
                                   created_at, updated_at)
             VALUES (?, (SELECT COUNT(*) FROM monitors WHERE node_id = ?), ?, ?, ?, '', '{}', ?,
                     ?, ?, ?, ?, ?, ?, 1, ?, 1, ?, ?)
             RETURNING id",
        )
        .bind(args.node.get())
        .bind(args.node.get())
        .bind(group.map(MonitorId::get))
        .bind(&valid.name)
        .bind(&valid.description)
        .bind(valid.interval)
        .bind(valid.retry_interval)
        .bind(valid.max_retries)
        .bind(valid.resend_interval)
        .bind(valid.timeout)
        .bind(valid.upside_down)
        .bind(to_text(&valid.quorum))
        .bind(serde_json::to_string(&valid.tags).map_err(ApiError::internal)?)
        .bind(now)
        .bind(now)
        .fetch_one(ctx.conn())
        .await?;
        let monitor = MonitorId::new(id);
        probes::assign(ctx.conn(), &scope, monitor, &valid.settings, &valid.probes).await?;
        notifications::link(ctx.conn(), &scope, monitor, channels.as_deref(), true).await?;
        settings::store(
            ctx.conn(),
            &self.secrets,
            monitor,
            valid.settings,
            settings::TokenSource::Person,
        )
        .await?;
        // A member of a group under maintenance is under maintenance from the start.
        maintenance::refresh_in(ctx).await?;
        record(ctx, args.node, "monitor.created", monitor).await?;
        answer(ctx, &self.secrets, monitor).await
    }

    async fn monitors_update(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorsUpdateArgs,
    ) -> Result<MonitorResult, ApiError> {
        let (row, scope) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        // Status changes share the topic, so edits compare the monitor's own revision.
        if let Some(expected) = args.expected_revision
            && expected != row.revision
        {
            return Err(ApiError::stale_revision(row.revision));
        }
        let stored = settings::stored_fields(ctx.conn(), args.monitor).await?;
        let group = args.input.group;
        let channels = args.input.channels.clone();
        let valid = settings::validate(args.input, &stored)?;
        if let Some(group) = group {
            check_group(ctx.conn(), &scope, group, Some(args.monitor)).await?;
        }
        let still_group = matches!(valid.settings, MonitorSettings::Group);
        if row.kind == to_text(&MonitorType::Group) && !still_group {
            let members: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM monitors WHERE group_id = ?")
                    .bind(args.monitor.get())
                    .fetch_one(ctx.conn())
                    .await?;
            if members > 0 {
                return Err(FieldErrors::one(
                    "settings",
                    FieldErrorCode::InvalidValue,
                    None,
                ));
            }
        }
        sqlx::query(
            "UPDATE monitors SET group_id = ?, name = ?, description = ?, interval = ?,
                                 retry_interval = ?, max_retries = ?, resend_interval = ?,
                                 timeout = ?, upside_down = ?, quorum = ?, tags = ?,
                                 revision = revision + 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(group.map(MonitorId::get))
        .bind(&valid.name)
        .bind(&valid.description)
        .bind(valid.interval)
        .bind(valid.retry_interval)
        .bind(valid.max_retries)
        .bind(valid.resend_interval)
        .bind(valid.timeout)
        .bind(valid.upside_down)
        .bind(to_text(&valid.quorum))
        .bind(serde_json::to_string(&valid.tags).map_err(ApiError::internal)?)
        .bind(ctx.now())
        .bind(args.monitor.get())
        .execute(ctx.conn())
        .await?;
        probes::assign(
            ctx.conn(),
            &scope,
            args.monitor,
            &valid.settings,
            &valid.probes,
        )
        .await?;
        notifications::link(ctx.conn(), &scope, args.monitor, channels.as_deref(), false).await?;
        settings::store(
            ctx.conn(),
            &self.secrets,
            args.monitor,
            valid.settings,
            settings::TokenSource::Person,
        )
        .await?;
        maintenance::refresh_in(ctx).await?;
        record(ctx, row.node(), "monitor.updated", args.monitor).await?;
        answer(ctx, &self.secrets, args.monitor).await
    }

    async fn monitors_pause(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorArgs,
    ) -> Result<MonitorResult, ApiError> {
        let (row, _) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        let paused = sqlx::query(
            "UPDATE monitors SET active = 0, revision = revision + 1, updated_at = ?
             WHERE id = ? AND active = 1",
        )
        .bind(ctx.now())
        .bind(args.monitor.get())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if paused > 0 {
            record(ctx, row.node(), "monitor.paused", args.monitor).await?;
        }
        answer(ctx, &self.secrets, args.monitor).await
    }

    async fn monitors_resume(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorArgs,
    ) -> Result<MonitorResult, ApiError> {
        let (row, _) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        if row.kind == to_text(&MonitorType::Unsupported) {
            return Err(FieldErrors::one(
                "monitor",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let resumed = sqlx::query(
            "UPDATE monitors SET active = 1, revision = revision + 1, updated_at = ?
             WHERE id = ? AND active = 0",
        )
        .bind(ctx.now())
        .bind(args.monitor.get())
        .execute(ctx.conn())
        .await?
        .rows_affected();
        if resumed > 0 {
            record(ctx, row.node(), "monitor.resumed", args.monitor).await?;
        }
        answer(ctx, &self.secrets, args.monitor).await
    }

    async fn monitors_set_manual_status(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorsSetManualStatusArgs,
    ) -> Result<MonitorResult, ApiError> {
        let (row, _) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        if row.kind != to_text(&MonitorType::Manual) {
            return Err(FieldErrors::one(
                "monitor",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let code = args.status.map(|status| {
            status_code(match status {
                ManualStatus::Up => Status::Up,
                ManualStatus::Down => Status::Down,
                ManualStatus::Pending => Status::Pending,
            })
        });
        // A new revision restarts the monitor's schedule, which records the status at once.
        sqlx::query(
            "UPDATE monitors SET manual_status = ?, revision = revision + 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(code)
        .bind(ctx.now())
        .bind(args.monitor.get())
        .execute(ctx.conn())
        .await?;
        record(ctx, row.node(), "monitor.updated", args.monitor).await?;
        answer(ctx, &self.secrets, args.monitor).await
    }

    async fn monitors_delete(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorArgs,
    ) -> Result<MonitorDeleted, ApiError> {
        let (row, _) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        // Pages lose the monitor with the row, so they are told first.
        pages::touch_in(ctx, Change::Monitor(args.monitor)).await?;
        // State, secrets and the journal cascade; members leave the group, and with it its
        // maintenance.
        sqlx::query("DELETE FROM monitors WHERE id = ?")
            .bind(args.monitor.get())
            .execute(ctx.conn())
            .await?;
        renumber(ctx.conn(), row.node(), None).await?;
        maintenance::refresh_in(ctx).await?;
        record(ctx, row.node(), "monitor.deleted", args.monitor).await?;
        Ok(MonitorDeleted {
            monitor: args.monitor,
        })
    }

    async fn monitors_move(
        &self,
        ctx: &mut CommandCtx,
        args: MonitorsMoveArgs,
    ) -> Result<MonitorResult, ApiError> {
        let (row, _) = locate(ctx, args.monitor, Capability::MonitorsManage).await?;
        let from = row.node();
        let to = ctx
            .node_scope(args.node, Capability::MonitorsManage)
            .await?;
        if let Some(group) = args.group {
            check_group(ctx.conn(), &to, group, Some(args.monitor)).await?;
        }
        let relocated = from != args.node;
        let moved = if relocated {
            with_members_on(ctx.conn(), args.monitor, from).await?
        } else {
            vec![args.monitor]
        };
        let now = ctx.now();
        if relocated || row.group_id != args.group.map(MonitorId::get) {
            sqlx::query(
                "UPDATE monitors SET node_id = ?, group_id = ?, revision = revision + 1,
                                     updated_at = ?
                 WHERE id = ?",
            )
            .bind(args.node.get())
            .bind(args.group.map(MonitorId::get))
            .bind(now)
            .bind(args.monitor.get())
            .execute(ctx.conn())
            .await?;
        }
        let mut detached = Vec::new();
        if relocated {
            let ids: Vec<i64> = moved.iter().map(|monitor| monitor.get()).collect();
            let ids = serde_json::to_string(&ids).map_err(ApiError::internal)?;
            sqlx::query(
                "UPDATE monitors SET node_id = ?1, revision = revision + 1, updated_at = ?2
                 WHERE id IN (SELECT value FROM json_each(?3)) AND id <> ?4",
            )
            .bind(args.node.get())
            .bind(now)
            .bind(&ids)
            .bind(args.monitor.get())
            .execute(ctx.conn())
            .await?;
            // Members elsewhere keep the group only while it sits on their node or above.
            let others: Vec<(i64, i64)> = sqlx::query_as(
                "SELECT id, node_id FROM monitors
                 WHERE group_id IN (SELECT value FROM json_each(?1))
                   AND id NOT IN (SELECT value FROM json_each(?1))",
            )
            .bind(&ids)
            .fetch_all(ctx.conn())
            .await?;
            for (member, node) in others {
                let reaches: Option<i64> =
                    sqlx::query_scalar(concat!(path_cte!(), "SELECT 1 FROM path WHERE id = ?2"))
                        .bind(node)
                        .bind(args.node.get())
                        .fetch_optional(ctx.conn())
                        .await?;
                if reaches.is_none() {
                    sqlx::query(
                        "UPDATE monitors SET group_id = NULL, revision = revision + 1,
                                             updated_at = ?
                         WHERE id = ?",
                    )
                    .bind(now)
                    .bind(member)
                    .execute(ctx.conn())
                    .await?;
                    detached.push((MonitorId::new(member), NodeId::new(node)));
                }
            }
            probes::prune_moved(ctx.conn(), &moved).await?;
            notifications::prune_moved(ctx.conn(), &moved).await?;
            maintenance::prune_moved(ctx.conn(), &moved).await?;
            pages::prune_moved(ctx).await?;
            renumber(ctx.conn(), from, None).await?;
        }
        renumber(ctx.conn(), args.node, Some((&moved, args.position))).await?;
        maintenance::refresh_in(ctx).await?;
        for (member, node) in detached {
            record(ctx, node, "monitor.updated", member).await?;
        }
        // The old node's topics first, so the new node's `monitors:<id>` answers the command.
        let mut topics = if relocated {
            event_topics(ctx.conn(), from).await?
        } else {
            Vec::new()
        };
        for topic in event_topics(ctx.conn(), args.node).await? {
            if !topics.contains(&topic) {
                topics.push(topic);
            }
        }
        for monitor in moved {
            pages::touch_in(ctx, Change::Monitor(monitor)).await?;
            let payload = json!({ "monitor": monitor, "node": args.node });
            for topic in &topics {
                ctx.record(topic, "monitor.moved", payload.clone()).await?;
            }
        }
        answer(ctx, &self.secrets, args.monitor).await
    }

    async fn monitors_list(
        &self,
        ctx: &mut QueryCtx,
        args: MonitorsListArgs,
    ) -> Result<Snapshot<MonitorList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        /// The summaries of monitors on the nodes `$nodes` selects, given node `?1`.
        macro_rules! summaries {
            ($nodes:literal) => {
                concat!(
                    "SELECT m.id, m.node_id, m.position, m.name, m.type, m.active, m.tags, m.group_id,
                            s.status, s.latency_ms, s.beat_at
                     FROM monitors m LEFT JOIN monitor_state s ON s.monitor_id = m.id
                     WHERE m.node_id IN (",
                    $nodes,
                    ") ORDER BY m.node_id, m.position, m.name, m.id"
                )
            };
        }
        // Reading a node means reading its whole subtree.
        let (sql, topic) = if args.subtree {
            (
                summaries!(
                    "WITH RECURSIVE below(id) AS (
                         SELECT ?1
                         UNION ALL
                         SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id
                     )
                     SELECT id FROM below"
                ),
                Topic::monitor_tree(args.node),
            )
        } else {
            (summaries!("SELECT ?1"), Topic::monitors(args.node))
        };
        let rows: Vec<SummaryRow> = sqlx::query_as(sql)
            .bind(args.node.get())
            .fetch_all(ctx.conn())
            .await?;
        let ids: Vec<MonitorId> = rows.iter().map(|row| MonitorId::new(row.id)).collect();
        let uptime = history::uptime_24h(&self.storage, &ids, self.clock.now_ms()).await?;
        let monitors = rows
            .into_iter()
            .map(|row| {
                let id = MonitorId::new(row.id);
                Ok(MonitorSummary {
                    id,
                    node: NodeId::new(row.node_id),
                    position: row.position as u32,
                    name: row.name,
                    kind: from_text(&row.kind).map_err(ApiError::internal)?,
                    active: row.active,
                    tags: serde_json::from_str(&row.tags).map_err(ApiError::internal)?,
                    group: row.group_id.map(MonitorId::new),
                    status: row.status.map(stored_status).transpose()?,
                    latency_ms: row.latency_ms.map(|ms| ms as u32),
                    beat_at: row.beat_at,
                    uptime_24h: uptime.get(&id).copied(),
                })
            })
            .collect::<Result<_, ApiError>>()?;
        Ok(Snapshot {
            topic,
            data: MonitorList { monitors },
        })
    }

    async fn monitors_get(
        &self,
        ctx: &mut QueryCtx,
        args: MonitorArgs,
    ) -> Result<Snapshot<MonitorDetail>, ApiError> {
        let row = load(ctx.conn(), args.monitor).await?;
        let scope = ctx.node_scope(row.node(), Capability::NodeRead).await?;
        let state: Option<(i64, String, Option<i64>, i64, i64, i64)> = sqlx::query_as(
            "SELECT status, message, latency_ms, retries, beat_at, changed_at
             FROM monitor_state WHERE monitor_id = ?",
        )
        .bind(args.monitor.get())
        .fetch_optional(ctx.conn())
        .await?;
        let state = state
            .map(
                |(status, message, latency_ms, retries, beat_at, changed_at)| {
                    Ok::<_, ApiError>(MonitorState {
                        status: stored_status(status)?,
                        message,
                        latency_ms: latency_ms.map(|ms| ms as u32),
                        retries: retries as u32,
                        beat_at,
                        changed_at,
                    })
                },
            )
            .transpose()?;
        let probes: Vec<ProbeRow> = sqlx::query_as(
            "SELECT p.id, p.name, s.status, s.message, s.latency_ms, s.retries, s.checked_at,
                    s.received_at
             FROM probe_state s JOIN probes p ON p.id = s.probe_id
             JOIN monitor_probes mp ON mp.monitor_id = s.monitor_id AND mp.probe_id = s.probe_id
             WHERE s.monitor_id = ? ORDER BY p.id",
        )
        .bind(args.monitor.get())
        .fetch_all(ctx.conn())
        .await?;
        let live = live_probes(
            ctx.conn(),
            args.monitor,
            row.interval.max(row.retry_interval) * 1000,
            self.clock.now_ms(),
            self.started_at,
        )
        .await?;
        let probes = probes
            .into_iter()
            .map(|row| {
                let probe = ProbeId::new(row.id);
                Ok(ProbeState {
                    probe,
                    name: row.name,
                    live: live.iter().any(|live| live.probe() == probe),
                    status: stored_status(row.status)?,
                    message: row.message,
                    latency_ms: row.latency_ms.map(|ms| ms as u32),
                    retries: row.retries as u32,
                    checked_at: row.checked_at,
                    received_at: row.received_at,
                })
            })
            .collect::<Result<_, ApiError>>()?;
        let manages = scope.can(Capability::MonitorsManage);
        let monitor = present(ctx.conn(), &self.secrets, row, manages).await?;
        Ok(Snapshot {
            topic: Topic::monitors(scope.node()),
            data: MonitorDetail {
                monitor,
                state,
                probes,
            },
        })
    }

    async fn monitors_heartbeats(
        &self,
        ctx: &mut QueryCtx,
        args: HeartbeatsArgs,
    ) -> Result<Snapshot<HeartbeatPage>, ApiError> {
        let topic = readable(ctx, args.monitor).await?;
        Ok(Snapshot {
            topic,
            data: history::heartbeats(&self.storage, &args).await?,
        })
    }

    async fn monitors_uptime(
        &self,
        ctx: &mut QueryCtx,
        args: UptimeArgs,
    ) -> Result<Snapshot<MonitorUptime>, ApiError> {
        let topic = readable(ctx, args.monitor).await?;
        Ok(Snapshot {
            topic,
            data: history::uptime(&self.storage, &args, self.clock.now_ms()).await?,
        })
    }

    async fn monitors_latency(
        &self,
        ctx: &mut QueryCtx,
        args: LatencyArgs,
    ) -> Result<Snapshot<LatencySeries>, ApiError> {
        let topic = readable(ctx, args.monitor).await?;
        Ok(Snapshot {
            topic,
            data: history::latency(&self.storage, &args).await?,
        })
    }

    async fn monitors_trend(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<ScopeTrend>, ApiError> {
        let ids = monitors_below(ctx, args.node).await?;
        Ok(Snapshot {
            topic: Topic::monitor_tree(args.node),
            data: history::trend(&self.storage, &ids, self.clock.now_ms()).await?,
        })
    }

    async fn monitors_hourly(
        &self,
        ctx: &mut QueryCtx,
        args: NodeArgs,
    ) -> Result<Snapshot<MonitorHourlyList>, ApiError> {
        let ids = monitors_below(ctx, args.node).await?;
        Ok(Snapshot {
            topic: Topic::monitor_tree(args.node),
            data: history::hourly(&self.storage, &ids, self.clock.now_ms()).await?,
        })
    }

    async fn monitors_day_bars(
        &self,
        ctx: &mut QueryCtx,
        args: DayBarsArgs,
    ) -> Result<Snapshot<DayBarList>, ApiError> {
        let topic = readable(ctx, args.monitor).await?;
        Ok(Snapshot {
            topic,
            data: history::day_bars(&self.storage, &args, self.clock.now_ms()).await?,
        })
    }
}
