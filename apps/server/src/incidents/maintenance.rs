//! Planned maintenance (decision 0004, 0008 item 9): `maintenance.*`, and which monitors are
//! under maintenance now.
//!
//! Schedules are Kuma's columns, computed by `v1per_maintenance_schedule`. [`refresh`] applies
//! them: it writes the monitors under an active window, and the members of covered groups, to
//! `maintenance_state`, and announces windows that start or end. Commands that change a
//! maintenance or a monitor's group refresh in their own transaction; [`run`] refreshes at the
//! next window edge `next_change` reports. Probes skip monitors in `maintenance_state` and the
//! built-in probe records their maintenance beats instead, so the change reaches the evaluator
//! through the assignment version.

use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::watch;
use v1per_maintenance_schedule::{self as schedule, Definition, Status};
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{MaintenanceId, MonitorId, NodeId};
use v1per_protocol::operations::maintenance::{
    Maintenance, MaintenanceArgs, MaintenanceCreateArgs, MaintenanceDeleted, MaintenanceDetail,
    MaintenanceHandlers, MaintenanceInput, MaintenanceList, MaintenanceListArgs,
    MaintenancePreview, MaintenanceResult, MaintenanceSchedule, MaintenanceStatus,
    MaintenanceStrategy, MaintenanceUpdateArgs, MaintenanceWindow,
};
use v1per_protocol::registry::Snapshot;

use super::{record_tree, record_tree_ctx, subtree_contains, text};
use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::instance;
use crate::pages::{self, Change};
use crate::state::AppState;

const TITLE_MAX: usize = 200;
const DESCRIPTION_MAX: usize = 10_000;
const LIST_WINDOWS: usize = 3;
const DETAIL_WINDOWS: usize = 10;
/// The longest the timer sleeps, in case a change was missed.
const IDLE_MS: i64 = 60 * 60_000;
/// Stored dates: Kuma's format.
const STORED_DATE: &str = "%Y-%m-%d %H:%M:%S";
/// Dates on the wire.
const WIRE_DATE: &str = "%Y-%m-%dT%H:%M";

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    title: String,
    description: String,
    strategy: String,
    active: bool,
    start_date: Option<String>,
    end_date: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    weekdays: String,
    days_of_month: String,
    interval_day: Option<i64>,
    cron: Option<String>,
    duration: Option<i64>,
    timezone: Option<String>,
    status: String,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

macro_rules! select_row {
    () => {
        "SELECT id, node_id, title, description, strategy, active, start_date, end_date,
                start_time, end_time, weekdays, days_of_month, interval_day, cron, duration,
                timezone, status, revision, created_at, updated_at
         FROM maintenances"
    };
}

impl Row {
    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }

    /// The schedule as `v1per_maintenance_schedule` reads it. `None` only for a broken row.
    fn definition(&self, server: Tz) -> Option<Definition> {
        let mut definition: Definition = serde_json::from_value(json!({
            "strategy": self.strategy,
            "active": self.active,
            "start_date": self.start_date,
            "end_date": self.end_date,
            "start_time": self.start_time,
            "end_time": self.end_time,
            "weekdays": self.weekdays,
            "days_of_month": self.days_of_month,
            "interval_day": self.interval_day,
            "cron": self.cron,
            "duration": self.duration,
            "timezone": self.timezone,
        }))
        .ok()?;
        definition.server_timezone = server;
        Some(definition)
    }

    fn schedule(&self) -> MaintenanceSchedule {
        let wire = |date: &Option<String>| {
            date.as_deref().map(|date| {
                NaiveDateTime::parse_from_str(date, STORED_DATE)
                    .map(|date| date.format(WIRE_DATE).to_string())
                    .unwrap_or_else(|_| date.to_owned())
            })
        };
        let days: Vec<Value> = serde_json::from_str(&self.days_of_month).unwrap_or_default();
        MaintenanceSchedule {
            strategy: serde_json::from_value(Value::String(self.strategy.clone()))
                .unwrap_or(MaintenanceStrategy::Manual),
            start_date: wire(&self.start_date),
            end_date: wire(&self.end_date),
            start_time: self.start_time.clone(),
            end_time: self.end_time.clone(),
            weekdays: serde_json::from_str(&self.weekdays).unwrap_or_default(),
            days_of_month: days
                .iter()
                .filter_map(|day| match day {
                    Value::Number(n) => n.as_u64().map(|n| n as u8),
                    Value::String(s) => s.parse().ok(),
                    _ => None,
                })
                .collect(),
            last_day_of_month: days.iter().any(|day| day == "lastDay1"),
            interval_day: self.interval_day.map(|days| days as u32),
            cron: self.cron.clone(),
            duration: self.duration.map(|seconds| seconds as u32),
            timezone: self.timezone.clone(),
        }
    }
}

fn wire_status(status: Status) -> MaintenanceStatus {
    match status {
        Status::Inactive => MaintenanceStatus::Inactive,
        Status::Scheduled => MaintenanceStatus::Scheduled,
        Status::UnderMaintenance => MaintenanceStatus::UnderMaintenance,
        Status::Ended => MaintenanceStatus::Ended,
        Status::Unknown => MaintenanceStatus::Unknown,
    }
}

fn status_text(status: MaintenanceStatus) -> String {
    match serde_json::to_value(status) {
        Ok(Value::String(text)) => text,
        _ => unreachable!("unit enums serialize as strings"),
    }
}

fn window(window: schedule::Window) -> MaintenanceWindow {
    let edge = |at: DateTime<Utc>| {
        (at != DateTime::<Utc>::MIN_UTC && at != DateTime::<Utc>::MAX_UTC)
            .then(|| at.timestamp_millis())
    };
    MaintenanceWindow {
        start: edge(window.start),
        end: edge(window.end),
    }
}

fn utc(ms: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

/// Up to `count` windows that end after `now`.
fn upcoming(definition: &Definition, now: i64, count: usize) -> Vec<MaintenanceWindow> {
    schedule::windows(definition, utc(now), DateTime::<Utc>::MAX_UTC)
        .take(count)
        .map(window)
        .collect()
}

/// The columns a valid schedule is stored in.
struct Stored {
    strategy: String,
    start_date: Option<String>,
    end_date: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    weekdays: String,
    days_of_month: String,
    interval_day: Option<i64>,
    cron: Option<String>,
    duration: Option<i64>,
    timezone: Option<String>,
}

/// Checks a schedule field by field, as `schedule.<field>` under `prefix`, and returns the
/// columns to store and the definition they make.
fn validate_schedule(
    schedule: &MaintenanceSchedule,
    prefix: &str,
    server: Tz,
) -> Result<(Stored, Definition), ApiError> {
    let mut errors = FieldErrors::default();
    let field = |name: &str| format!("{prefix}{name}");
    let date = |errors: &mut FieldErrors, name: &str, value: &Option<String>| {
        let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty())?;
        let parsed = ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S"]
            .iter()
            .find_map(|format| NaiveDateTime::parse_from_str(value, format).ok());
        if parsed.is_none() {
            errors.add(&field(name), FieldErrorCode::InvalidFormat, None);
        }
        parsed
    };
    let time = |errors: &mut FieldErrors, name: &str, value: &Option<String>| {
        let value = value.as_deref().map(str::trim).filter(|v| !v.is_empty())?;
        let valid = chrono::NaiveTime::parse_from_str(value, "%H:%M").is_ok();
        if !valid {
            errors.add(&field(name), FieldErrorCode::InvalidFormat, None);
        }
        valid.then(|| value.to_owned())
    };
    let start_date = date(&mut errors, "start_date", &schedule.start_date);
    let end_date = date(&mut errors, "end_date", &schedule.end_date);
    if let (Some(start), Some(end)) = (start_date, end_date)
        && end <= start
    {
        errors.add(&field("end_date"), FieldErrorCode::InvalidValue, None);
    }
    let start_time = time(&mut errors, "start_time", &schedule.start_time);
    let end_time = time(&mut errors, "end_time", &schedule.end_time);
    let timezone = schedule
        .timezone
        .as_deref()
        .map(str::trim)
        .filter(|zone| !zone.is_empty());
    if let Some(zone) = timezone
        && zone.parse::<Tz>().is_err()
    {
        errors.add(&field("timezone"), FieldErrorCode::InvalidValue, None);
    }
    let given = |value: &Option<String>| value.as_deref().is_some_and(|v| !v.trim().is_empty());
    let mut require = |name: &str, present: bool| {
        if !present {
            errors.add(&field(name), FieldErrorCode::Required, None);
        }
    };
    use MaintenanceStrategy as S;
    match schedule.strategy {
        S::Manual => {}
        S::Single => {
            require("start_date", given(&schedule.start_date));
            require("end_date", given(&schedule.end_date));
        }
        S::RecurringInterval | S::RecurringWeekday | S::RecurringDayOfMonth => {
            require("start_time", given(&schedule.start_time));
            require("end_time", given(&schedule.end_time));
            match schedule.strategy {
                S::RecurringInterval => {
                    require("start_date", given(&schedule.start_date));
                    require("interval_day", schedule.interval_day.is_some());
                }
                S::RecurringWeekday => require("weekdays", !schedule.weekdays.is_empty()),
                _ => require(
                    "days_of_month",
                    !schedule.days_of_month.is_empty() || schedule.last_day_of_month,
                ),
            }
        }
        S::Cron => {
            require(
                "cron",
                schedule
                    .cron
                    .as_deref()
                    .is_some_and(|c| !c.trim().is_empty()),
            );
            require("duration", schedule.duration.is_some_and(|d| d > 0));
        }
    }
    if schedule
        .interval_day
        .is_some_and(|days| !(1..=3650).contains(&days))
    {
        errors.add(&field("interval_day"), FieldErrorCode::InvalidValue, None);
    }
    if schedule.weekdays.iter().any(|day| *day > 7) {
        errors.add(&field("weekdays"), FieldErrorCode::InvalidValue, None);
    }
    if schedule
        .days_of_month
        .iter()
        .any(|day| !(1..=31).contains(day))
    {
        errors.add(&field("days_of_month"), FieldErrorCode::InvalidValue, None);
    }
    errors.finish()?;

    let mut days: Vec<Value> = schedule.days_of_month.iter().map(|d| json!(d)).collect();
    if schedule.last_day_of_month {
        days.push(json!("lastDay1"));
    }
    let stored = Stored {
        strategy: serde_json::to_value(schedule.strategy)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .expect("unit enums serialize as strings"),
        start_date: start_date.map(|d| d.format(STORED_DATE).to_string()),
        end_date: end_date.map(|d| d.format(STORED_DATE).to_string()),
        start_time,
        end_time,
        weekdays: serde_json::to_string(&schedule.weekdays).expect("numbers serialize"),
        days_of_month: Value::Array(days).to_string(),
        interval_day: schedule.interval_day.map(i64::from),
        cron: schedule.cron.as_deref().map(str::trim).map(str::to_owned),
        duration: schedule.duration.map(i64::from),
        timezone: timezone.map(str::to_owned),
    };
    let row = Row {
        id: 0,
        node_id: 0,
        title: String::new(),
        description: String::new(),
        strategy: stored.strategy.clone(),
        active: true,
        start_date: stored.start_date.clone(),
        end_date: stored.end_date.clone(),
        start_time: stored.start_time.clone(),
        end_time: stored.end_time.clone(),
        weekdays: stored.weekdays.clone(),
        days_of_month: stored.days_of_month.clone(),
        interval_day: stored.interval_day,
        cron: stored.cron.clone(),
        duration: stored.duration,
        timezone: stored.timezone.clone(),
        status: String::new(),
        revision: 0,
        created_at: 0,
        updated_at: 0,
    };
    let definition = row
        .definition(server)
        .ok_or_else(|| ApiError::internal("a validated schedule does not parse"))?;
    if let Err(error) = definition.validate() {
        let name = match error {
            schedule::Error::Cron(_) => "cron",
            schedule::Error::Empty(name) | schedule::Error::Missing(name) => {
                name.split(' ').next().unwrap_or("schedule")
            }
        };
        return Err(FieldErrors::one(
            &field(name),
            FieldErrorCode::InvalidFormat,
            None,
        ));
    }
    Ok((stored, definition))
}

/// A maintenance whose window started or ended.
pub(crate) struct StatusChange {
    pub node: NodeId,
    pub maintenance: MaintenanceId,
}

impl StatusChange {
    fn payload(&self) -> Value {
        json!({ "maintenance": self.maintenance })
    }
}

/// `covered(id)`: the monitors of the maintenances in the JSON list `?1`, and the members of
/// covered groups at any depth.
macro_rules! covered_cte {
    () => {
        "WITH RECURSIVE covered(id) AS (
             SELECT mm.monitor_id FROM maintenance_monitors mm
             WHERE mm.maintenance_id IN (SELECT value FROM json_each(?1))
             UNION
             SELECT m.id FROM monitors m JOIN covered c ON m.group_id = c.id
         ) "
    };
}

/// Brings `maintenance_state` and each maintenance's announced status in line with `now`.
/// Returns the windows that started or ended, for the caller to announce, and the next
/// instant anything may change.
pub(crate) async fn refresh(
    conn: &mut SqliteConnection,
    now: i64,
) -> sqlx::Result<(Vec<StatusChange>, Option<i64>)> {
    let server = instance::timezone(conn).await?;
    let rows: Vec<Row> = sqlx::query_as(select_row!()).fetch_all(&mut *conn).await?;
    let mut active = Vec::new();
    let mut next: Option<i64> = None;
    let mut changes = Vec::new();
    for row in rows {
        let definition = row.definition(server);
        let status = definition.as_ref().map_or(MaintenanceStatus::Unknown, |d| {
            wire_status(schedule::status_at(d, utc(now)))
        });
        if status == MaintenanceStatus::UnderMaintenance {
            active.push(row.id);
        }
        if let Some(change) = definition
            .as_ref()
            .and_then(|d| schedule::next_change(d, utc(now)))
        {
            let change = change.timestamp_millis();
            next = Some(next.map_or(change, |next| next.min(change)));
        }
        let text = status_text(status);
        if text != row.status {
            sqlx::query("UPDATE maintenances SET status = ? WHERE id = ?")
                .bind(&text)
                .bind(row.id)
                .execute(&mut *conn)
                .await?;
            changes.push(StatusChange {
                node: row.node(),
                maintenance: MaintenanceId::new(row.id),
            });
        }
    }
    let ids = serde_json::to_string(&active).expect("numbers serialize");
    sqlx::query(concat!(
        covered_cte!(),
        "DELETE FROM maintenance_state WHERE monitor_id NOT IN (SELECT id FROM covered)"
    ))
    .bind(&ids)
    .execute(&mut *conn)
    .await?;
    sqlx::query(concat!(
        covered_cte!(),
        "INSERT OR IGNORE INTO maintenance_state (monitor_id) SELECT id FROM covered"
    ))
    .bind(&ids)
    .execute(&mut *conn)
    .await?;
    Ok((changes, next))
}

/// [`refresh`] inside a command, announcing what changed through it.
pub(crate) async fn refresh_in(ctx: &mut CommandCtx) -> Result<(), ApiError> {
    let now = ctx.now();
    let (changes, _) = refresh(ctx.conn(), now).await?;
    for change in changes {
        pages::touch_in(ctx, Change::Maintenance(change.maintenance)).await?;
        record_tree_ctx(
            ctx,
            Topic::maintenance,
            change.node,
            "maintenance.status_changed",
            change.payload(),
        )
        .await?;
    }
    Ok(())
}

/// Refreshes at every window edge until `stop`. Changes to maintenances and the instance
/// timezone wake it early.
pub async fn run(state: AppState, mut stop: watch::Receiver<bool>) {
    let (mut wake, _) = state.hub.subscribe();
    while !*stop.borrow() {
        let now = state.clock.now_ms();
        let next = match tick(&state, now).await {
            Ok(next) => next,
            Err(error) => {
                tracing::error!(%error, "applying maintenance windows failed; retrying");
                Some(now + 5_000)
            }
        };
        let until = next.map_or(now + IDLE_MS, |next| next.min(now + IDLE_MS));
        loop {
            tokio::select! {
                woken = wake.recv() => match woken {
                    Ok(topic) if topic.starts_with("maintenance:") || &*topic == "instance" => break,
                    Ok(_) | Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => return,
                },
                _ = state.clock.sleep_until(until) => break,
                _ = stop.changed() => break,
            }
        }
    }
}

async fn tick(state: &AppState, now: i64) -> anyhow::Result<Option<i64>> {
    let mut tx = state.db.writer.begin_with("BEGIN IMMEDIATE").await?;
    let (changes, next) = refresh(&mut tx, now).await?;
    let mut topics: Vec<Topic> = Vec::new();
    for change in changes {
        for topic in pages::touch(&mut tx, Change::Maintenance(change.maintenance), now).await? {
            if !topics.contains(&topic) {
                topics.push(topic);
            }
        }
        let recorded = record_tree(
            &mut tx,
            Topic::maintenance,
            change.node,
            "maintenance.status_changed",
            &change.payload(),
            now,
        )
        .await?;
        for topic in recorded {
            if !topics.contains(&topic) {
                topics.push(topic);
            }
        }
    }
    tx.commit().await?;
    for topic in &topics {
        state.hub.publish(topic);
    }
    Ok(next)
}

/// A maintenance as status pages show it.
pub(crate) struct Shown {
    pub title: String,
    pub description: String,
    pub active: bool,
    pub window: MaintenanceWindow,
    /// The monitors it covers among those asked about.
    pub monitors: Vec<MonitorId>,
}

/// Maintenances covering any of `monitors`, directly or through a covered group, that are
/// under way or open a window before `until`.
pub(crate) async fn shown_for(
    conn: &mut SqliteConnection,
    monitors: &[MonitorId],
    now: i64,
    until: i64,
) -> sqlx::Result<Vec<Shown>> {
    if monitors.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = monitors.iter().map(|monitor| monitor.get()).collect();
    let covered: Vec<(i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE covered(maintenance_id, id) AS (
             SELECT maintenance_id, monitor_id FROM maintenance_monitors
             UNION
             SELECT c.maintenance_id, m.id FROM monitors m JOIN covered c ON m.group_id = c.id
         )
         SELECT maintenance_id, id FROM covered
         WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY maintenance_id, id",
    )
    .bind(serde_json::to_string(&ids).expect("numbers serialize"))
    .fetch_all(&mut *conn)
    .await?;
    let server = instance::timezone(conn).await?;
    let mut shown = Vec::new();
    for maintenance in covered
        .iter()
        .map(|(m, _)| *m)
        .collect::<std::collections::BTreeSet<_>>()
    {
        let row: Row = sqlx::query_as(concat!(select_row!(), " WHERE id = ?"))
            .bind(maintenance)
            .fetch_one(&mut *conn)
            .await?;
        let Some(definition) = row.definition(server) else {
            continue;
        };
        let active = schedule::status_at(&definition, utc(now)) == Status::UnderMaintenance;
        let window = upcoming(&definition, now, 1)
            .pop()
            .unwrap_or(MaintenanceWindow {
                start: None,
                end: None,
            });
        let soon = row.active && window.start.is_some_and(|start| start < until);
        if !active && !soon {
            continue;
        }
        shown.push(Shown {
            title: row.title,
            description: row.description,
            active,
            window,
            monitors: covered
                .iter()
                .filter(|(m, _)| *m == maintenance)
                .map(|(_, monitor)| MonitorId::new(*monitor))
                .collect(),
        });
    }
    Ok(shown)
}

/// After `node` moved: drops monitors that left the subtree of the maintenances covering them.
pub(crate) async fn prune_moved(conn: &mut SqliteConnection, node: NodeId) -> sqlx::Result<()> {
    let rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE moved(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN moved ON n.parent_id = moved.id
         )
         SELECT mm.maintenance_id, mm.monitor_id, x.node_id, m.node_id
         FROM maintenance_monitors mm
         JOIN maintenances x ON x.id = mm.maintenance_id
         JOIN monitors m ON m.id = mm.monitor_id
         WHERE m.node_id IN (SELECT id FROM moved)",
    )
    .bind(node.get())
    .fetch_all(&mut *conn)
    .await?;
    for (maintenance, monitor, owner, monitor_node) in rows {
        if !subtree_contains(conn, NodeId::new(owner), NodeId::new(monitor_node)).await? {
            sqlx::query(
                "DELETE FROM maintenance_monitors WHERE maintenance_id = ? AND monitor_id = ?",
            )
            .bind(maintenance)
            .bind(monitor)
            .execute(&mut *conn)
            .await?;
        }
    }
    Ok(())
}

async fn load(conn: &mut SqliteConnection, maintenance: MaintenanceId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE id = ?"))
        .bind(maintenance.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

async fn present(
    conn: &mut SqliteConnection,
    row: Row,
    now: i64,
    windows: usize,
) -> Result<Maintenance, ApiError> {
    let server = instance::timezone(conn).await?;
    let monitors: Vec<i64> = sqlx::query_scalar(
        "SELECT monitor_id FROM maintenance_monitors WHERE maintenance_id = ? ORDER BY monitor_id",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let definition = row.definition(server);
    let status = definition.as_ref().map_or(MaintenanceStatus::Unknown, |d| {
        wire_status(schedule::status_at(d, utc(now)))
    });
    let windows = definition
        .as_ref()
        .map(|d| upcoming(d, now, windows))
        .unwrap_or_default();
    Ok(Maintenance {
        id: MaintenanceId::new(row.id),
        node: row.node(),
        schedule: row.schedule(),
        monitors: monitors.into_iter().map(MonitorId::new).collect(),
        active: row.active,
        status,
        windows,
        title: row.title,
        description: row.description,
        revision: row.revision,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Validates the input against `node` and stores schedule and monitors for `maintenance`.
async fn store(
    ctx: &mut CommandCtx,
    node: NodeId,
    maintenance: Option<MaintenanceId>,
    input: &MaintenanceInput,
) -> Result<MaintenanceId, ApiError> {
    let title = text("title", &input.title, true, TITLE_MAX)?;
    let description = text("description", &input.description, false, DESCRIPTION_MAX)?;
    let server = instance::timezone(ctx.conn()).await?;
    let (stored, _) = validate_schedule(&input.schedule, "schedule.", server)?;
    let mut monitors: Vec<i64> = input.monitors.iter().map(|m| m.get()).collect();
    monitors.sort_unstable();
    monitors.dedup();
    for monitor in &monitors {
        let owner: Option<i64> = sqlx::query_scalar("SELECT node_id FROM monitors WHERE id = ?")
            .bind(monitor)
            .fetch_optional(ctx.conn())
            .await?;
        let inside = match owner {
            Some(owner) => subtree_contains(ctx.conn(), node, NodeId::new(owner)).await?,
            None => false,
        };
        if !inside {
            return Err(FieldErrors::one(
                "monitors",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
    }
    let now = ctx.now();
    let id = match maintenance {
        None => sqlx::query_scalar(
            "INSERT INTO maintenances (node_id, title, description, strategy, active, start_date,
                 end_date, start_time, end_time, weekdays, days_of_month, interval_day, cron,
                 duration, timezone, revision, created_at, updated_at)
             VALUES (?, ?, ?, ?, 1, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?) RETURNING id",
        )
        .bind(node.get())
        .bind(&title)
        .bind(&description)
        .bind(&stored.strategy)
        .bind(&stored.start_date)
        .bind(&stored.end_date)
        .bind(&stored.start_time)
        .bind(&stored.end_time)
        .bind(&stored.weekdays)
        .bind(&stored.days_of_month)
        .bind(stored.interval_day)
        .bind(&stored.cron)
        .bind(stored.duration)
        .bind(&stored.timezone)
        .bind(now)
        .bind(now)
        .fetch_one(ctx.conn())
        .await?,
        Some(maintenance) => {
            sqlx::query(
                "UPDATE maintenances SET title = ?, description = ?, strategy = ?, start_date = ?,
                     end_date = ?, start_time = ?, end_time = ?, weekdays = ?, days_of_month = ?,
                     interval_day = ?, cron = ?, duration = ?, timezone = ?,
                     revision = revision + 1, updated_at = ?
                 WHERE id = ?",
            )
            .bind(&title)
            .bind(&description)
            .bind(&stored.strategy)
            .bind(&stored.start_date)
            .bind(&stored.end_date)
            .bind(&stored.start_time)
            .bind(&stored.end_time)
            .bind(&stored.weekdays)
            .bind(&stored.days_of_month)
            .bind(stored.interval_day)
            .bind(&stored.cron)
            .bind(stored.duration)
            .bind(&stored.timezone)
            .bind(now)
            .bind(maintenance.get())
            .execute(ctx.conn())
            .await?;
            maintenance.get()
        }
    };
    sqlx::query("DELETE FROM maintenance_monitors WHERE maintenance_id = ?")
        .bind(id)
        .execute(ctx.conn())
        .await?;
    for monitor in monitors {
        sqlx::query("INSERT INTO maintenance_monitors (maintenance_id, monitor_id) VALUES (?, ?)")
            .bind(id)
            .bind(monitor)
            .execute(ctx.conn())
            .await?;
    }
    Ok(MaintenanceId::new(id))
}

/// Records `kind` on the maintenance topics and applies the windows.
async fn changed(
    ctx: &mut CommandCtx,
    node: NodeId,
    kind: &str,
    maintenance: MaintenanceId,
) -> Result<(), ApiError> {
    pages::touch_in(ctx, Change::Maintenance(maintenance)).await?;
    let payload = json!({ "maintenance": maintenance });
    record_tree_ctx(ctx, Topic::maintenance, node, kind, payload).await?;
    refresh_in(ctx).await
}

async fn answer(
    ctx: &mut CommandCtx,
    maintenance: MaintenanceId,
) -> Result<MaintenanceResult, ApiError> {
    let row = load(ctx.conn(), maintenance).await?;
    let now = ctx.now();
    Ok(MaintenanceResult {
        maintenance: present(ctx.conn(), row, now, DETAIL_WINDOWS).await?,
    })
}

impl MaintenanceHandlers for AppState {
    async fn maintenance_create(
        &self,
        ctx: &mut CommandCtx,
        args: MaintenanceCreateArgs,
    ) -> Result<MaintenanceResult, ApiError> {
        ctx.node_scope(args.node, Capability::MaintenanceManage)
            .await?;
        let id = store(ctx, args.node, None, &args.maintenance).await?;
        changed(ctx, args.node, "maintenance.created", id).await?;
        answer(ctx, id).await
    }

    async fn maintenance_update(
        &self,
        ctx: &mut CommandCtx,
        args: MaintenanceUpdateArgs,
    ) -> Result<MaintenanceResult, ApiError> {
        let row = load(ctx.conn(), args.maintenance).await?;
        ctx.node_scope(row.node(), Capability::MaintenanceManage)
            .await?;
        if let Some(expected) = args.expected_revision
            && expected != row.revision
        {
            return Err(ApiError::stale_revision(row.revision));
        }
        store(ctx, row.node(), Some(args.maintenance), &args.input).await?;
        changed(ctx, row.node(), "maintenance.updated", args.maintenance).await?;
        answer(ctx, args.maintenance).await
    }

    async fn maintenance_pause(
        &self,
        ctx: &mut CommandCtx,
        args: MaintenanceArgs,
    ) -> Result<MaintenanceResult, ApiError> {
        set_active(ctx, args.maintenance, false).await
    }

    async fn maintenance_resume(
        &self,
        ctx: &mut CommandCtx,
        args: MaintenanceArgs,
    ) -> Result<MaintenanceResult, ApiError> {
        set_active(ctx, args.maintenance, true).await
    }

    async fn maintenance_delete(
        &self,
        ctx: &mut CommandCtx,
        args: MaintenanceArgs,
    ) -> Result<MaintenanceDeleted, ApiError> {
        let row = load(ctx.conn(), args.maintenance).await?;
        ctx.node_scope(row.node(), Capability::MaintenanceManage)
            .await?;
        // Pages lose the covered monitors with the row, so they are told first.
        pages::touch_in(ctx, Change::Maintenance(args.maintenance)).await?;
        sqlx::query("DELETE FROM maintenances WHERE id = ?")
            .bind(args.maintenance.get())
            .execute(ctx.conn())
            .await?;
        changed(ctx, row.node(), "maintenance.deleted", args.maintenance).await?;
        Ok(MaintenanceDeleted {
            maintenance: args.maintenance,
        })
    }

    async fn maintenance_list(
        &self,
        ctx: &mut QueryCtx,
        args: MaintenanceListArgs,
    ) -> Result<Snapshot<MaintenanceList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        let rows: Vec<Row> = sqlx::query_as(concat!(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id WHERE ?2
             ) ",
            select_row!(),
            " WHERE node_id IN (SELECT id FROM below) ORDER BY title, id"
        ))
        .bind(args.node.get())
        .bind(args.subtree)
        .fetch_all(ctx.conn())
        .await?;
        let now = self.clock.now_ms();
        let mut maintenances = Vec::with_capacity(rows.len());
        for row in rows {
            maintenances.push(present(ctx.conn(), row, now, LIST_WINDOWS).await?);
        }
        Ok(Snapshot {
            topic: Topic::maintenance(args.node),
            data: MaintenanceList { maintenances },
        })
    }

    async fn maintenance_get(
        &self,
        ctx: &mut QueryCtx,
        args: MaintenanceArgs,
    ) -> Result<Snapshot<MaintenanceDetail>, ApiError> {
        let row = load(ctx.conn(), args.maintenance).await?;
        let node = row.node();
        ctx.node_scope(node, Capability::NodeRead).await?;
        let maintenance = present(ctx.conn(), row, self.clock.now_ms(), DETAIL_WINDOWS).await?;
        Ok(Snapshot {
            topic: Topic::maintenance(node),
            data: MaintenanceDetail { maintenance },
        })
    }

    async fn maintenance_preview(
        &self,
        ctx: &mut QueryCtx,
        args: MaintenanceSchedule,
    ) -> Result<Snapshot<MaintenancePreview>, ApiError> {
        let server = instance::timezone(ctx.conn()).await?;
        let (_, definition) = validate_schedule(&args, "", server)?;
        Ok(Snapshot {
            topic: Topic::instance(),
            data: MaintenancePreview {
                windows: upcoming(&definition, self.clock.now_ms(), DETAIL_WINDOWS),
            },
        })
    }
}

async fn set_active(
    ctx: &mut CommandCtx,
    maintenance: MaintenanceId,
    active: bool,
) -> Result<MaintenanceResult, ApiError> {
    let row = load(ctx.conn(), maintenance).await?;
    ctx.node_scope(row.node(), Capability::MaintenanceManage)
        .await?;
    if row.active != active {
        sqlx::query(
            "UPDATE maintenances SET active = ?, revision = revision + 1, updated_at = ?
             WHERE id = ?",
        )
        .bind(active)
        .bind(ctx.now())
        .bind(maintenance.get())
        .execute(ctx.conn())
        .await?;
        changed(ctx, row.node(), "maintenance.updated", maintenance).await?;
    }
    answer(ctx, maintenance).await
}
