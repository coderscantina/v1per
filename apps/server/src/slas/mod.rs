//! SLAs (decision 0012, milestone 14): `slas.*`, the checks behind `pages.updateSlas`, and
//! keeping an SLA's monitors and channels in reach when nodes or monitors move. Periods are
//! computed in [`period`]; [`run`] tallies days and freezes periods in the background, which
//! is one query per tick on an install without SLAs.
//!
//! Reading needs `node.read` on the SLA's node. Writes need Admin through a grant on a strict
//! ancestor, or the instance admin (`NodeScope::require_admin_above`), so a client Admin on
//! the SLA's own node reads only.

mod job;
pub mod period;

pub use job::{catch_up, run};

use chrono::{NaiveDate, NaiveTime, Timelike};
use chrono_tz::Tz;
use serde_json::{Value, json};
use sqlx::SqliteConnection;
use v1per_protocol::access::Capability;
use v1per_protocol::envelope::Topic;
use v1per_protocol::errors::FieldErrorCode;
use v1per_protocol::ids::{ChannelId, MonitorId, NodeId, PageId, SlaId};
use v1per_protocol::operations::slas::{
    Sla, SlaArgs, SlaDeleted, SlaDetail, SlaHours, SlaInput, SlaList, SlaPeriod, SlaPeriodArgs,
    SlaPeriodResult, SlaResult, SlaTimeTarget, SlaTimeTargets, SlasCreateArgs, SlasHandlers,
    SlasListArgs, SlasUpdateArgs,
};
use v1per_protocol::registry::Snapshot;

use crate::context::{CommandCtx, QueryCtx};
use crate::error::{ApiError, FieldErrors};
use crate::incidents::{record_tree_ctx, text};
use crate::nodes::scope::path_cte;
use crate::state::AppState;

const NAME_MAX: usize = 100;
const MONITORS_MAX: usize = 500;
const HOLIDAYS_MAX: usize = 1_000;
const HOURS_MAX: usize = 100;
/// A year of business minutes.
const MINUTES_MAX: u32 = 525_600;
const LATENCY_MAX: u32 = 600_000;
/// SLAs one status page shows.
const PAGE_SLAS_MAX: usize = 20;
const DATE: &str = "%Y-%m-%d";
const IMPACTS: [&str; 3] = ["major_outage", "partial_outage", "degraded"];

/// `below(id)`: node `?1` and every node under it.
macro_rules! below_cte {
    () => {
        "WITH RECURSIVE below(id) AS (
             SELECT ?1
             UNION ALL
             SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id
         ) "
    };
}

/// `up(start, id)`: every node paired with itself and each of its ancestors.
macro_rules! up_cte {
    () => {
        "WITH RECURSIVE up(start, id) AS (
             SELECT id, id FROM nodes
             UNION ALL
             SELECT up.start, n.parent_id FROM nodes n JOIN up ON n.id = up.id
             WHERE n.parent_id IS NOT NULL
         ) "
    };
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: i64,
    name: String,
    timezone: String,
    period: String,
    start_date: String,
    combine: String,
    availability: Option<f64>,
    latency_ms: Option<i64>,
    revision: i64,
    created_at: i64,
    updated_at: i64,
}

macro_rules! select_row {
    () => {
        "SELECT id, node_id, name, timezone, period, start_date, combine, availability,
                latency_ms, revision, created_at, updated_at
         FROM slas"
    };
}

impl Row {
    fn node(&self) -> NodeId {
        NodeId::new(self.node_id)
    }

    fn sla(&self) -> SlaId {
        SlaId::new(self.id)
    }
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

fn json_ids(ids: &[i64]) -> String {
    serde_json::to_string(ids).expect("numbers serialize")
}

async fn load(conn: &mut SqliteConnection, sla: SlaId) -> Result<Row, ApiError> {
    sqlx::query_as(concat!(select_row!(), " WHERE id = ?"))
        .bind(sla.get())
        .fetch_optional(conn)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// `HH:MM` for a minute of the day, `24:00` for the end of it.
fn clock(minute: i64) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

async fn present(conn: &mut SqliteConnection, row: Row) -> Result<Sla, ApiError> {
    let targets: Vec<(String, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT impact, response_minutes, resolution_minutes FROM sla_time_targets
         WHERE sla_id = ?",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let mut times = SlaTimeTargets::default();
    for (impact, response, resolution) in targets {
        let target = SlaTimeTarget {
            response_minutes: response.map(|m| m as u32),
            resolution_minutes: resolution.map(|m| m as u32),
        };
        match impact.as_str() {
            "major_outage" => times.major_outage = target,
            "partial_outage" => times.partial_outage = target,
            _ => times.degraded = target,
        }
    }
    let hours: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT weekday, start_minute, end_minute FROM sla_hours WHERE sla_id = ?
         ORDER BY weekday, start_minute",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let holidays: Vec<String> =
        sqlx::query_scalar("SELECT day FROM sla_holidays WHERE sla_id = ? ORDER BY day")
            .bind(row.id)
            .fetch_all(&mut *conn)
            .await?;
    let monitors: Vec<i64> = sqlx::query_scalar(
        "SELECT monitor_id FROM sla_monitors WHERE sla_id = ? ORDER BY monitor_id",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    let channels: Vec<i64> = sqlx::query_scalar(
        "SELECT channel_id FROM sla_channels WHERE sla_id = ? ORDER BY channel_id",
    )
    .bind(row.id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Sla {
        id: row.sla(),
        node: row.node(),
        terms: SlaInput {
            period: enum_from(&row.period)?,
            combine: enum_from(&row.combine)?,
            name: row.name,
            timezone: row.timezone,
            start_date: row.start_date,
            availability: row.availability,
            latency_ms: row.latency_ms.map(|ms| ms as u32),
            times,
            hours: hours
                .into_iter()
                .map(|(weekday, start, end)| SlaHours {
                    weekday: weekday as u8,
                    start: clock(start),
                    end: clock(end),
                })
                .collect(),
            holidays,
            monitors: monitors.into_iter().map(MonitorId::new).collect(),
            channels: channels.into_iter().map(ChannelId::new).collect(),
        },
        revision: row.revision,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Terms that passed the field checks, ready to store.
struct Valid {
    name: String,
    timezone: String,
    start_date: String,
    /// `(impact, response, resolution)` for impacts with a target.
    times: Vec<(&'static str, Option<u32>, Option<u32>)>,
    /// `(weekday, start minute, end minute)`.
    hours: Vec<(u8, u32, u32)>,
    holidays: Vec<String>,
    monitors: Vec<i64>,
    channels: Vec<i64>,
}

/// A minute of the day from `HH:MM`, with `24:00` allowed when `end`.
fn minute(value: &str, end: bool) -> Option<u32> {
    let value = value.trim();
    if end && value == "24:00" {
        return Some(1440);
    }
    let time = NaiveTime::parse_from_str(value, "%H:%M").ok()?;
    Some(time.hour() * 60 + time.minute())
}

/// Checks every field that needs no database and collects all errors.
fn validate(input: &SlaInput) -> Result<Valid, ApiError> {
    let mut errors = FieldErrors::default();
    let name = text("name", &input.name, true, NAME_MAX)?;
    let timezone = input.timezone.trim().to_owned();
    if timezone.parse::<Tz>().is_err() {
        errors.add("timezone", FieldErrorCode::InvalidValue, None);
    }
    let start_date = input.start_date.trim().to_owned();
    if NaiveDate::parse_from_str(&start_date, DATE).is_err() {
        errors.add("start_date", FieldErrorCode::InvalidFormat, None);
    }
    if let Some(target) = input.availability {
        let thousandths = target * 1000.0;
        let valid = target.is_finite()
            && target > 0.0
            && target <= 100.0
            && (thousandths - thousandths.round()).abs() < 1e-6;
        if !valid {
            errors.add("availability", FieldErrorCode::InvalidValue, None);
        }
    }
    if input
        .latency_ms
        .is_some_and(|ms| !(1..=LATENCY_MAX).contains(&ms))
    {
        errors.add(
            "latency_ms",
            FieldErrorCode::InvalidValue,
            Some(LATENCY_MAX),
        );
    }
    let targets = [
        input.times.major_outage,
        input.times.partial_outage,
        input.times.degraded,
    ];
    let mut times = Vec::new();
    for (impact, target) in IMPACTS.into_iter().zip(targets) {
        for (field, minutes) in [
            ("response_minutes", target.response_minutes),
            ("resolution_minutes", target.resolution_minutes),
        ] {
            if minutes.is_some_and(|m| !(1..=MINUTES_MAX).contains(&m)) {
                errors.add(
                    &format!("times.{impact}.{field}"),
                    FieldErrorCode::InvalidValue,
                    Some(MINUTES_MAX),
                );
            }
        }
        if target.response_minutes.is_some() || target.resolution_minutes.is_some() {
            times.push((impact, target.response_minutes, target.resolution_minutes));
        }
    }
    let mut hours = Vec::new();
    if input.hours.len() > HOURS_MAX {
        errors.add("hours", FieldErrorCode::TooMany, Some(HOURS_MAX as u32));
    }
    for range in &input.hours {
        match (minute(&range.start, false), minute(&range.end, true)) {
            (Some(start), Some(end)) if range.weekday <= 6 && end > start => {
                hours.push((range.weekday, start, end));
            }
            _ => errors.add("hours", FieldErrorCode::InvalidValue, None),
        }
    }
    hours.sort_unstable();
    // Sorted by weekday and start, so an overlap (or a repeat) is a range starting before the
    // previous one ends.
    if hours
        .windows(2)
        .any(|pair| pair[0].0 == pair[1].0 && pair[1].1 < pair[0].2)
    {
        errors.add("hours", FieldErrorCode::InvalidValue, None);
    }
    let mut holidays = Vec::new();
    for day in &input.holidays {
        match NaiveDate::parse_from_str(day.trim(), DATE) {
            Ok(day) => holidays.push(day.format(DATE).to_string()),
            Err(_) => errors.add("holidays", FieldErrorCode::InvalidFormat, None),
        }
    }
    holidays.sort_unstable();
    holidays.dedup();
    if holidays.len() > HOLIDAYS_MAX {
        errors.add(
            "holidays",
            FieldErrorCode::TooMany,
            Some(HOLIDAYS_MAX as u32),
        );
    }
    let mut monitors: Vec<i64> = input.monitors.iter().map(|m| m.get()).collect();
    monitors.sort_unstable();
    monitors.dedup();
    if monitors.is_empty() {
        errors.add("monitors", FieldErrorCode::Required, None);
    } else if monitors.len() > MONITORS_MAX {
        errors.add(
            "monitors",
            FieldErrorCode::TooMany,
            Some(MONITORS_MAX as u32),
        );
    }
    let mut channels: Vec<i64> = input.channels.iter().map(|c| c.get()).collect();
    channels.sort_unstable();
    channels.dedup();
    errors.finish()?;
    Ok(Valid {
        name,
        timezone,
        start_date,
        times,
        hours,
        holidays,
        monitors,
        channels,
    })
}

/// `invalid_value` on `monitors` unless each is on `node` or below, and on `channels` unless
/// each is on `node` or an ancestor, readable or not.
async fn check_reach(
    conn: &mut SqliteConnection,
    node: NodeId,
    valid: &Valid,
) -> Result<(), ApiError> {
    let mut errors = FieldErrors::default();
    let monitors: i64 = sqlx::query_scalar(concat!(
        below_cte!(),
        "SELECT count(*) FROM monitors
         WHERE id IN (SELECT value FROM json_each(?2)) AND node_id IN (SELECT id FROM below)"
    ))
    .bind(node.get())
    .bind(json_ids(&valid.monitors))
    .fetch_one(&mut *conn)
    .await?;
    if monitors as usize != valid.monitors.len() {
        errors.add("monitors", FieldErrorCode::InvalidValue, None);
    }
    let channels: i64 = sqlx::query_scalar(concat!(
        path_cte!(),
        "SELECT count(*) FROM channels
         WHERE id IN (SELECT value FROM json_each(?2)) AND node_id IN (SELECT id FROM path)"
    ))
    .bind(node.get())
    .bind(json_ids(&valid.channels))
    .fetch_one(&mut *conn)
    .await?;
    if channels as usize != valid.channels.len() {
        errors.add("channels", FieldErrorCode::InvalidValue, None);
    }
    errors.finish()
}

/// Validates `input` against `node` and stores it as `sla`, or as a new SLA.
async fn store(
    ctx: &mut CommandCtx,
    node: NodeId,
    sla: Option<SlaId>,
    input: &SlaInput,
) -> Result<SlaId, ApiError> {
    let valid = validate(input)?;
    check_reach(ctx.conn(), node, &valid).await?;
    let now = ctx.now();
    let period = enum_text(input.period);
    let combine = enum_text(input.combine);
    let latency = input.latency_ms.map(i64::from);
    let id: i64 = match sla {
        None => {
            sqlx::query_scalar(
                "INSERT INTO slas (node_id, name, timezone, period, start_date, combine,
                     availability, latency_ms, revision, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?) RETURNING id",
            )
            .bind(node.get())
            .bind(&valid.name)
            .bind(&valid.timezone)
            .bind(&period)
            .bind(&valid.start_date)
            .bind(&combine)
            .bind(input.availability)
            .bind(latency)
            .bind(now)
            .bind(now)
            .fetch_one(ctx.conn())
            .await?
        }
        Some(sla) => {
            sqlx::query(
                "UPDATE slas SET name = ?, timezone = ?, period = ?, start_date = ?, combine = ?,
                     availability = ?, latency_ms = ?, revision = revision + 1, updated_at = ?
                 WHERE id = ?",
            )
            .bind(&valid.name)
            .bind(&valid.timezone)
            .bind(&period)
            .bind(&valid.start_date)
            .bind(&combine)
            .bind(input.availability)
            .bind(latency)
            .bind(now)
            .bind(sla.get())
            .execute(ctx.conn())
            .await?;
            for delete in [
                "DELETE FROM sla_time_targets WHERE sla_id = ?",
                "DELETE FROM sla_hours WHERE sla_id = ?",
                "DELETE FROM sla_holidays WHERE sla_id = ?",
                "DELETE FROM sla_monitors WHERE sla_id = ?",
                "DELETE FROM sla_channels WHERE sla_id = ?",
            ] {
                sqlx::query(delete)
                    .bind(sla.get())
                    .execute(ctx.conn())
                    .await?;
            }
            sla.get()
        }
    };
    for (impact, response, resolution) in &valid.times {
        sqlx::query(
            "INSERT INTO sla_time_targets (sla_id, impact, response_minutes, resolution_minutes)
             VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(impact)
        .bind(response.map(i64::from))
        .bind(resolution.map(i64::from))
        .execute(ctx.conn())
        .await?;
    }
    for (weekday, start, end) in &valid.hours {
        sqlx::query(
            "INSERT INTO sla_hours (sla_id, weekday, start_minute, end_minute) VALUES (?, ?, ?, ?)",
        )
        .bind(id)
        .bind(i64::from(*weekday))
        .bind(i64::from(*start))
        .bind(i64::from(*end))
        .execute(ctx.conn())
        .await?;
    }
    let lists: [(&str, Value); 3] = [
        (
            "INSERT INTO sla_holidays (sla_id, day) SELECT ?1, value FROM json_each(?2)",
            json!(valid.holidays),
        ),
        (
            "INSERT INTO sla_monitors (sla_id, monitor_id) SELECT ?1, value FROM json_each(?2)",
            json!(valid.monitors),
        ),
        (
            "INSERT INTO sla_channels (sla_id, channel_id) SELECT ?1, value FROM json_each(?2)",
            json!(valid.channels),
        ),
    ];
    for (insert, values) in lists {
        sqlx::query(insert)
            .bind(id)
            .bind(values.to_string())
            .execute(ctx.conn())
            .await?;
    }
    Ok(SlaId::new(id))
}

/// The caller's scope on `node` when they may change its SLAs: `not_found` when they cannot
/// read it, `forbidden` without Admin through a strict ancestor.
async fn writable(ctx: &mut CommandCtx, node: NodeId) -> Result<(), ApiError> {
    ctx.node_scope(node, Capability::NodeRead)
        .await?
        .require_admin_above()
}

/// Tells the pages showing `sla` that their snapshot changed.
async fn touch_pages(ctx: &mut CommandCtx, sla: SlaId) -> Result<(), ApiError> {
    let pages: Vec<i64> = sqlx::query_scalar("SELECT page_id FROM page_slas WHERE sla_id = ?")
        .bind(sla.get())
        .fetch_all(ctx.conn())
        .await?;
    for page in pages {
        ctx.record_also(
            &Topic::page(PageId::new(page)),
            "page.status_changed",
            json!({}),
        )
        .await?;
    }
    Ok(())
}

async fn answer(ctx: &mut CommandCtx, sla: SlaId) -> Result<SlaResult, ApiError> {
    let row = load(ctx.conn(), sla).await?;
    Ok(SlaResult {
        sla: present(ctx.conn(), row).await?,
    })
}

/// After a node or monitor move: drops covered monitors that left their SLA's subtree,
/// channels no longer on its path, and page selections of SLAs no longer below the page.
pub(crate) async fn prune_moved(ctx: &mut CommandCtx) -> Result<(), ApiError> {
    let any: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM slas)")
        .fetch_one(ctx.conn())
        .await?;
    if !any {
        return Ok(());
    }
    let mut changed: Vec<i64> = sqlx::query_scalar(concat!(
        up_cte!(),
        "DELETE FROM sla_monitors WHERE (sla_id, monitor_id) IN (
             SELECT sm.sla_id, sm.monitor_id FROM sla_monitors sm
             JOIN slas s ON s.id = sm.sla_id JOIN monitors m ON m.id = sm.monitor_id
             WHERE NOT EXISTS (SELECT 1 FROM up WHERE up.start = m.node_id AND up.id = s.node_id)
         ) RETURNING sla_id"
    ))
    .fetch_all(ctx.conn())
    .await?;
    changed.extend(
        sqlx::query_scalar::<_, i64>(concat!(
            up_cte!(),
            "DELETE FROM sla_channels WHERE (sla_id, channel_id) IN (
                 SELECT sc.sla_id, sc.channel_id FROM sla_channels sc
                 JOIN slas s ON s.id = sc.sla_id JOIN channels c ON c.id = sc.channel_id
                 WHERE NOT EXISTS (SELECT 1 FROM up WHERE up.start = s.node_id AND up.id = c.node_id)
             ) RETURNING sla_id"
        ))
        .fetch_all(ctx.conn())
        .await?,
    );
    let pages: Vec<i64> = sqlx::query_scalar(concat!(
        up_cte!(),
        "DELETE FROM page_slas WHERE (page_id, sla_id) IN (
             SELECT ps.page_id, ps.sla_id FROM page_slas ps
             JOIN pages p ON p.id = ps.page_id JOIN slas s ON s.id = ps.sla_id
             WHERE NOT EXISTS (SELECT 1 FROM up WHERE up.start = s.node_id AND up.id = p.node_id)
         ) RETURNING page_id"
    ))
    .fetch_all(ctx.conn())
    .await?;
    changed.sort_unstable();
    changed.dedup();
    for sla in changed {
        sqlx::query("UPDATE slas SET revision = revision + 1, updated_at = ? WHERE id = ?")
            .bind(ctx.now())
            .bind(sla)
            .execute(ctx.conn())
            .await?;
        let row = load(ctx.conn(), SlaId::new(sla)).await?;
        record_tree_ctx(
            ctx,
            Topic::slas,
            row.node(),
            "sla.updated",
            json!({ "sla": row.sla() }),
        )
        .await?;
    }
    for page in pages {
        ctx.record_also(&Topic::page(PageId::new(page)), "page.updated", json!({}))
            .await?;
    }
    Ok(())
}

/// The SLAs a page on `node` shows, in order: each on the node or below and listed once, at
/// most [`PAGE_SLAS_MAX`]. `invalid_value` or `too_many` on `slas` otherwise.
pub(crate) async fn check_page_selection(
    conn: &mut SqliteConnection,
    node: NodeId,
    slas: &[SlaId],
) -> Result<Vec<i64>, ApiError> {
    if slas.len() > PAGE_SLAS_MAX {
        return Err(FieldErrors::one(
            "slas",
            FieldErrorCode::TooMany,
            Some(PAGE_SLAS_MAX as u32),
        ));
    }
    let ids: Vec<i64> = slas.iter().map(|sla| sla.get()).collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    let found: i64 = sqlx::query_scalar(concat!(
        below_cte!(),
        "SELECT count(*) FROM slas
         WHERE id IN (SELECT value FROM json_each(?2)) AND node_id IN (SELECT id FROM below)"
    ))
    .bind(node.get())
    .bind(json_ids(&unique))
    .fetch_one(conn)
    .await?;
    if unique.len() != ids.len() || found as usize != ids.len() {
        return Err(FieldErrors::one("slas", FieldErrorCode::InvalidValue, None));
    }
    Ok(ids)
}

impl SlasHandlers for AppState {
    async fn slas_create(
        &self,
        ctx: &mut CommandCtx,
        args: SlasCreateArgs,
    ) -> Result<SlaResult, ApiError> {
        writable(ctx, args.node).await?;
        let sla = store(ctx, args.node, None, &args.sla).await?;
        record_tree_ctx(
            ctx,
            Topic::slas,
            args.node,
            "sla.created",
            json!({ "sla": sla }),
        )
        .await?;
        answer(ctx, sla).await
    }

    async fn slas_update(
        &self,
        ctx: &mut CommandCtx,
        args: SlasUpdateArgs,
    ) -> Result<SlaResult, ApiError> {
        let row = load(ctx.conn(), args.sla).await?;
        writable(ctx, row.node()).await?;
        if let Some(expected) = args.expected_revision
            && expected != row.revision
        {
            return Err(ApiError::stale_revision(row.revision));
        }
        store(ctx, row.node(), Some(args.sla), &args.input).await?;
        touch_pages(ctx, args.sla).await?;
        record_tree_ctx(
            ctx,
            Topic::slas,
            row.node(),
            "sla.updated",
            json!({ "sla": args.sla }),
        )
        .await?;
        answer(ctx, args.sla).await
    }

    async fn slas_delete(
        &self,
        ctx: &mut CommandCtx,
        args: SlaArgs,
    ) -> Result<SlaDeleted, ApiError> {
        let row = load(ctx.conn(), args.sla).await?;
        writable(ctx, row.node()).await?;
        // Pages lose the SLA with the row, so they are told first.
        touch_pages(ctx, args.sla).await?;
        sqlx::query("DELETE FROM slas WHERE id = ?")
            .bind(args.sla.get())
            .execute(ctx.conn())
            .await?;
        record_tree_ctx(
            ctx,
            Topic::slas,
            row.node(),
            "sla.deleted",
            json!({ "sla": args.sla }),
        )
        .await?;
        Ok(SlaDeleted { sla: args.sla })
    }

    /// `required` on `start` without it, `invalid_value` for a period that is not frozen
    /// yet: it ended less than an hour ago or is still running.
    async fn slas_recompute(
        &self,
        ctx: &mut CommandCtx,
        args: SlaPeriodArgs,
    ) -> Result<SlaPeriodResult, ApiError> {
        let row = load(ctx.conn(), args.sla).await?;
        writable(ctx, row.node()).await?;
        let Some(start) = args.start.as_deref() else {
            return Err(FieldErrors::one("start", FieldErrorCode::Required, None));
        };
        let sla = present(ctx.conn(), row).await?;
        let now = ctx.now();
        let calendar = period::calendar(&sla.terms)?;
        let due = period::resolve(&calendar, Some(start), now)?;
        if due.ends_at.timestamp_millis() + period::FREEZE_DELAY_MS > now {
            return Err(FieldErrors::one(
                "start",
                FieldErrorCode::InvalidValue,
                None,
            ));
        }
        let mut result = period::compute(ctx.conn(), &self.storage, &sla, &due, now).await?;
        result.frozen_at = Some(now);
        sqlx::query(
            "INSERT INTO sla_periods (sla_id, start, starts_at, ends_at, result, frozen_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (sla_id, start) DO UPDATE SET starts_at = excluded.starts_at,
                 ends_at = excluded.ends_at, result = excluded.result,
                 frozen_at = excluded.frozen_at",
        )
        .bind(sla.id.get())
        .bind(&result.start)
        .bind(result.starts_at)
        .bind(result.ends_at)
        .bind(serde_json::to_string(&result).map_err(ApiError::internal)?)
        .bind(now)
        .execute(ctx.conn())
        .await?;
        touch_pages(ctx, sla.id).await?;
        record_tree_ctx(
            ctx,
            Topic::slas,
            sla.node,
            "sla.period_frozen",
            json!({ "sla": sla.id, "start": result.start }),
        )
        .await?;
        Ok(SlaPeriodResult { period: result })
    }

    async fn slas_list(
        &self,
        ctx: &mut QueryCtx,
        args: SlasListArgs,
    ) -> Result<Snapshot<SlaList>, ApiError> {
        ctx.node_scope(args.node, Capability::NodeRead).await?;
        let rows: Vec<Row> = sqlx::query_as(concat!(
            "WITH RECURSIVE below(id) AS (
                 SELECT ?1
                 UNION ALL
                 SELECT n.id FROM nodes n JOIN below b ON n.parent_id = b.id WHERE ?2
             ) ",
            select_row!(),
            " WHERE node_id IN (SELECT id FROM below) ORDER BY name, id"
        ))
        .bind(args.node.get())
        .bind(args.subtree)
        .fetch_all(ctx.conn())
        .await?;
        let mut slas = Vec::with_capacity(rows.len());
        for row in rows {
            slas.push(present(ctx.conn(), row).await?);
        }
        Ok(Snapshot {
            topic: Topic::slas(args.node),
            data: SlaList { slas },
        })
    }

    async fn slas_get(
        &self,
        ctx: &mut QueryCtx,
        args: SlaArgs,
    ) -> Result<Snapshot<SlaDetail>, ApiError> {
        let row = load(ctx.conn(), args.sla).await?;
        let node = row.node();
        ctx.node_scope(node, Capability::NodeRead).await?;
        Ok(Snapshot {
            topic: Topic::slas(node),
            data: SlaDetail {
                sla: present(ctx.conn(), row).await?,
            },
        })
    }

    /// The stored result of a frozen period, a computation as of now otherwise.
    async fn slas_period(
        &self,
        ctx: &mut QueryCtx,
        args: SlaPeriodArgs,
    ) -> Result<Snapshot<SlaPeriodResult>, ApiError> {
        let row = load(ctx.conn(), args.sla).await?;
        let node = row.node();
        ctx.node_scope(node, Capability::NodeRead).await?;
        let sla = present(ctx.conn(), row).await?;
        let now = self.clock.now_ms();
        let calendar = period::calendar(&sla.terms)?;
        let due = period::resolve(&calendar, args.start.as_deref(), now)?;
        let frozen: Option<String> =
            sqlx::query_scalar("SELECT result FROM sla_periods WHERE sla_id = ? AND start = ?")
                .bind(sla.id.get())
                .bind(due.first_day.format(DATE).to_string())
                .fetch_optional(ctx.conn())
                .await?;
        let period = match frozen {
            Some(result) => {
                serde_json::from_str::<SlaPeriod>(&result).map_err(ApiError::internal)?
            }
            None => period::compute(ctx.conn(), &self.storage, &sla, &due, now).await?,
        };
        Ok(Snapshot {
            topic: Topic::slas(node),
            data: SlaPeriodResult { period },
        })
    }
}
