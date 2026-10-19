//! Page snapshots per audience, the color rules of the monitoring contract, and a small cache.
//!
//! A snapshot reads the control database in one read transaction and the heartbeat store in
//! its own snapshot. Public audiences get status, bars, uptime, published incidents and
//! maintenance of the components not marked internal; the internal audience also gets every
//! component, hourly response times and per-probe status. Monitor, component, node and person
//! IDs, monitor targets and check messages never enter a snapshot.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Days, NaiveDate, Offset};
use chrono_tz::Tz;
use sqlx::SqliteConnection;
use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{IncidentId, MonitorId, PageComponentId, PageGroupId};
use v1per_protocol::limits::DAY_BARS_MAX;
use v1per_protocol::operations::incidents::{Impact, IncidentState};
use v1per_protocol::operations::monitors::LatencyPoint;
use v1per_protocol::status_page::{
    ComponentDetail, Locale, PageAppearance, PageAudience, PageBar, PageIncidents, PageSnapshot,
    PageStatus, ProbeView, PublicComponent, PublicGroup, PublicImpact, PublicIncident,
    PublicIncidentUpdate, PublicMaintenance,
};
use v1per_storage::{self as storage, Counts, ReadError, Resolution, Series};

use super::{PageRow, enum_from, logo, text};
use crate::error::ApiError;
use crate::events;
use crate::incidents::maintenance;
use crate::monitors::{status_from_code, wire_status};
use crate::state::AppState;

const DAY_MS: i64 = 86_400_000;
const HOUR_MS: i64 = 3_600_000;
/// Upcoming maintenance shows this far ahead.
const MAINTENANCE_AHEAD_MS: i64 = 14 * DAY_MS;
/// The span of per-minute response times for monitors without two hourly points yet.
const RECENT_MS: i64 = 2 * HOUR_MS;
/// Published incidents a snapshot lists at most.
const INCIDENTS_MAX: i64 = 100;
/// A cached snapshot is rebuilt after this long even when nothing it reads moved, so the day
/// boundary and maintenance windows follow the clock.
const CACHE_TTL_MS: i64 = 60_000;
/// Above this many entries the cache starts over.
const CACHE_MAX: usize = 512;

/// The color of a monitor's status: down is a major outage, pending degraded.
fn monitor_color(code: i64) -> Option<PageStatus> {
    status_from_code(code).map(|status| match status {
        storage::Status::Down => PageStatus::MajorOutage,
        storage::Status::Pending => PageStatus::Degraded,
        storage::Status::Up => PageStatus::Operational,
        storage::Status::Maintenance => PageStatus::Maintenance,
    })
}

fn impact_color(impact: Impact) -> PageStatus {
    match impact {
        Impact::Operational => PageStatus::Operational,
        Impact::Degraded => PageStatus::Degraded,
        Impact::PartialOutage => PageStatus::PartialOutage,
        Impact::MajorOutage => PageStatus::MajorOutage,
    }
}

/// A component's color now (monitoring contract, Component color and page status): the worst
/// of its monitor's status and the impact of open published incidents naming it. Maintenance
/// overrides the monitor's status but not a published incident. A static component without
/// incidents is operational; a monitor without status has none.
pub(super) fn component_status(
    monitor: Option<PageStatus>,
    has_monitor: bool,
    maintenance: bool,
    incident: Option<Impact>,
) -> Option<PageStatus> {
    let incident = incident
        .filter(|impact| *impact > Impact::Operational)
        .map(impact_color);
    if maintenance {
        return Some(incident.unwrap_or(PageStatus::Maintenance));
    }
    let base = if has_monitor {
        monitor
    } else {
        Some(PageStatus::Operational)
    };
    match (base, incident) {
        (Some(base), Some(incident)) => Some(base.max(incident)),
        (base, incident) => base.or(incident),
    }
}

/// What a daily bar is colored from.
pub(super) struct Day {
    pub uptime: Option<f64>,
    /// Some heartbeats that day were under maintenance.
    pub maintenance: bool,
    /// The worst impact published incidents gave the component that day.
    pub incident: Option<Impact>,
    pub has_monitor: bool,
    /// A static component existed that day; before it was created the day has no status.
    pub existed: bool,
}

/// The color of one daily bar. Decision 0008 item 13 is proposed and waits for Mike: a
/// published incident's impact wins; otherwise uptime colors the day, 100 % operational, at
/// least 99 % degraded, at least 95 % partial outage, below that major outage, and
/// maintenance shows when the day was otherwise operational. Change the rule here only.
pub(super) fn bar_status(day: &Day) -> Option<PageStatus> {
    if let Some(impact) = day.incident.filter(|impact| *impact > Impact::Operational) {
        return Some(impact_color(impact));
    }
    if !day.has_monitor {
        return day.existed.then_some(PageStatus::Operational);
    }
    let by_uptime = day.uptime.map(|uptime| match uptime {
        u if u >= 1.0 => PageStatus::Operational,
        u if u >= 0.99 => PageStatus::Degraded,
        u if u >= 0.95 => PageStatus::PartialOutage,
        _ => PageStatus::MajorOutage,
    });
    match (by_uptime, day.maintenance) {
        (Some(PageStatus::Operational) | None, true) => Some(PageStatus::Maintenance),
        (by_uptime, _) => by_uptime,
    }
}

/// The local day of `ms` in `zone`, with a non-hour offset's minutes dropped as the bars
/// count them (decision 0008 item 11).
fn local_day(ms: i64, zone: Tz) -> NaiveDate {
    let utc = DateTime::from_timestamp_millis(ms).unwrap_or_default();
    let offset = utc.with_timezone(&zone).offset().fix().local_minus_utc();
    (utc + chrono::Duration::seconds(i64::from(offset / 3600 * 3600))).date_naive()
}

/// Rounds a ratio to six places, so snapshots stay small.
fn ratio(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

fn uptime_of(counts: &Counts) -> Option<f64> {
    counts.uptime().map(ratio)
}

fn read_failed(error: ReadError) -> ApiError {
    ApiError::internal(error)
}

/// A built snapshot and its JSON.
#[derive(Clone)]
pub(super) struct Built {
    pub snapshot: Arc<PageSnapshot>,
    pub json: Arc<str>,
}

struct Entry {
    head: i64,
    /// The heartbeat watermark sequence before the build read bars and uptime.
    watermark: i64,
    built_at: i64,
    built: Built,
}

/// Built snapshots per page, audience and locale, valid while the page's topic stays at the
/// same head and no heartbeat became readable since, for at most a minute.
#[derive(Default)]
pub struct SnapshotCache(Mutex<HashMap<(i64, PageAudience, Locale), Entry>>);

/// The snapshot of `page` for `audience` in `locale`, from the cache when still current.
pub(super) async fn cached(
    state: &AppState,
    page: &PageRow,
    audience: PageAudience,
    locale: Locale,
) -> Result<Built, ApiError> {
    let now = state.clock.now_ms();
    // Read before the build, so a beat that becomes readable during it invalidates the entry.
    let watermark = state.storage.watermark().borrow().sequence;
    let head = {
        let mut conn = state.db.reader.acquire().await?;
        events::head(&mut conn, &Topic::page(page.page())).await?
    };
    let key = (page.id, audience, locale);
    {
        let cache = state.page_snapshots.0.lock().expect("snapshot cache");
        if let Some(entry) = cache.get(&key)
            && entry.head == head
            && entry.watermark == watermark
            && now - entry.built_at < CACHE_TTL_MS
        {
            return Ok(entry.built.clone());
        }
    }
    let snapshot = build(state, page, audience, locale, now).await?;
    let json: Arc<str> = serde_json::to_string(&snapshot)
        .map_err(ApiError::internal)?
        .into();
    let built = Built {
        snapshot: Arc::new(snapshot),
        json,
    };
    let mut cache = state.page_snapshots.0.lock().expect("snapshot cache");
    if cache.len() >= CACHE_MAX {
        cache.clear();
    }
    cache.insert(
        key,
        Entry {
            head,
            watermark,
            built_at: now,
            built: built.clone(),
        },
    );
    Ok(built)
}

pub(super) fn appearance(page: &PageRow) -> PageAppearance {
    PageAppearance {
        title: page.title.clone(),
        theme: page.theme(),
        accent: page.accent(),
        logo: logo::path(page),
        default_locale: page.default_locale(),
        locales: page.locales(),
    }
}

#[derive(sqlx::FromRow)]
struct ComponentRow {
    id: i64,
    group_id: i64,
    monitor_id: Option<i64>,
    component_id: Option<i64>,
    name: String,
    description: String,
    active: Option<bool>,
    status: Option<i64>,
    in_maintenance: bool,
    /// When a static component was created.
    created_at: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct IncidentRow {
    id: i64,
    title: String,
    title_kind: Option<String>,
    state: String,
    auto: bool,
    auto_monitor_id: Option<i64>,
    started_at: i64,
    resolved_at: Option<i64>,
}

/// A published incident as one page shows it, with what the bars and statuses need.
struct Published {
    incident: PublicIncident,
    /// Current impact per page component.
    current: HashMap<PageComponentId, Impact>,
    /// Worst impact per page component over the incident's life.
    peak: HashMap<PageComponentId, Impact>,
}

macro_rules! select_incident {
    () => {
        "SELECT i.id, i.title, i.title_kind, i.state, i.auto, i.auto_monitor_id, i.started_at,
                i.resolved_at
         FROM incident_publications p JOIN incidents i ON i.id = p.incident_id"
    };
}

/// The page's components `internal` decides between, in layout order.
async fn component_rows(
    conn: &mut SqliteConnection,
    page: &PageRow,
    internal: bool,
) -> sqlx::Result<Vec<ComponentRow>> {
    sqlx::query_as(
        "SELECT pc.id, pc.group_id, pc.monitor_id, pc.component_id,
                coalesce(pc.display_name, m.name, c.name, '') AS name,
                coalesce(c.description, '') AS description,
                m.active, s.status, ms.monitor_id IS NOT NULL AS in_maintenance,
                c.created_at
         FROM page_components pc
         LEFT JOIN monitors m ON m.id = pc.monitor_id
         LEFT JOIN components c ON c.id = pc.component_id
         LEFT JOIN monitor_state s ON s.monitor_id = pc.monitor_id
         LEFT JOIN maintenance_state ms ON ms.monitor_id = pc.monitor_id
         WHERE pc.page_id = ?1 AND (?2 OR pc.internal = 0)
         ORDER BY pc.position, pc.id",
    )
    .bind(page.id)
    .bind(internal)
    .fetch_all(conn)
    .await
}

/// The page components an audience sees, by what they show, and their names.
struct Shown {
    by_monitor: HashMap<i64, PageComponentId>,
    by_component: HashMap<i64, PageComponentId>,
    names: HashMap<PageComponentId, String>,
}

impl Shown {
    fn new(rows: &[ComponentRow]) -> Self {
        Self {
            by_monitor: rows
                .iter()
                .filter_map(|row| Some((row.monitor_id?, PageComponentId::new(row.id))))
                .collect(),
            by_component: rows
                .iter()
                .filter_map(|row| Some((row.component_id?, PageComponentId::new(row.id))))
                .collect(),
            names: rows
                .iter()
                .map(|row| (PageComponentId::new(row.id), row.name.clone()))
                .collect(),
        }
    }

    /// The page component showing an incident target, if visible.
    fn of(&self, monitor: Option<i64>, component: Option<i64>) -> Option<PageComponentId> {
        match (monitor, component) {
            (Some(monitor), _) => self.by_monitor.get(&monitor).copied(),
            (_, Some(component)) => self.by_component.get(&component).copied(),
            _ => None,
        }
    }
}

/// Published incidents as the page shows them in `locale`. An incident naming no shown
/// component is left out.
async fn published(
    conn: &mut SqliteConnection,
    incidents: Vec<IncidentRow>,
    shown: &Shown,
    locale: Locale,
) -> Result<Vec<Published>, ApiError> {
    let mut published = Vec::new();
    for row in incidents {
        let targets: Vec<(Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT monitor_id, component_id FROM incident_targets
             WHERE incident_id = ? ORDER BY rowid",
        )
        .bind(row.id)
        .fetch_all(&mut *conn)
        .await?;
        let components: Vec<PageComponentId> = targets
            .iter()
            .filter_map(|(monitor, component)| shown.of(*monitor, *component))
            .collect();
        if components.is_empty() {
            continue;
        }
        let updates: Vec<(i64, String, String, bool, i64)> = sqlx::query_as(
            "SELECT id, state, message, author_id IS NULL, created_at FROM incident_updates
             WHERE incident_id = ? ORDER BY id DESC",
        )
        .bind(row.id)
        .fetch_all(&mut *conn)
        .await?;
        let impacts: Vec<(i64, Option<i64>, Option<i64>, String)> = sqlx::query_as(
            "SELECT i.update_id, i.monitor_id, i.component_id, i.impact
             FROM incident_update_impacts i JOIN incident_updates u ON u.id = i.update_id
             WHERE u.incident_id = ? ORDER BY u.id, i.rowid",
        )
        .bind(row.id)
        .fetch_all(&mut *conn)
        .await?;
        let mut current: HashMap<PageComponentId, Impact> = HashMap::new();
        let mut peak: HashMap<PageComponentId, Impact> = HashMap::new();
        let mut per_update: HashMap<i64, Vec<PublicImpact>> = HashMap::new();
        for (update, monitor, component, impact) in &impacts {
            let Some(id) = shown.of(*monitor, *component) else {
                continue;
            };
            let impact: Impact = enum_from(impact)?;
            current.insert(id, impact);
            let worst = peak.entry(id).or_insert(impact);
            *worst = (*worst).max(impact);
            per_update.entry(*update).or_default().push(PublicImpact {
                component: id,
                impact,
            });
        }
        if row.resolved_at.is_some() {
            for id in &components {
                current.insert(*id, Impact::Operational);
            }
        }
        let auto_name = row
            .auto_monitor_id
            .and_then(|monitor| shown.by_monitor.get(&monitor))
            .and_then(|id| shown.names.get(id))
            .map(String::as_str);
        let title = match row.title_kind.as_deref() {
            Some("monitor_down") => text::monitor_down(locale, auto_name),
            _ => row.title.clone(),
        };
        let updates = updates
            .into_iter()
            .map(|(id, state, message, by_server, created_at)| {
                let state: IncidentState = enum_from(&state)?;
                let message = if row.auto && by_server {
                    text::auto_update(locale, state, auto_name)
                } else {
                    message
                };
                Ok(PublicIncidentUpdate {
                    state,
                    message,
                    created_at,
                    impacts: per_update.remove(&id).unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        published.push(Published {
            incident: PublicIncident {
                id: IncidentId::new(row.id),
                title,
                state: enum_from(&row.state)?,
                impact: components
                    .iter()
                    .filter_map(|id| current.get(id))
                    .copied()
                    .max(),
                started_at: row.started_at,
                resolved_at: row.resolved_at,
                components,
                updates,
            },
            current,
            peak,
        });
    }
    Ok(published)
}

async fn build(
    state: &AppState,
    page: &PageRow,
    audience: PageAudience,
    locale: Locale,
    now: i64,
) -> Result<PageSnapshot, ApiError> {
    let internal = audience == PageAudience::Internal;
    let zone = page.zone();
    let today = local_day(now, zone);
    let first = today
        .checked_sub_days(Days::new(u64::from(DAY_BARS_MAX - 1)))
        .unwrap_or(today);
    let window_start = now - i64::from(DAY_BARS_MAX + 1) * DAY_MS;

    let mut tx = state.db.reader.begin().await?;
    let head = events::head(&mut tx, &Topic::page(page.page())).await?;
    let groups: Vec<(i64, String, bool)> = sqlx::query_as(
        "SELECT id, name, default_open FROM page_groups WHERE page_id = ? ORDER BY position",
    )
    .bind(page.id)
    .fetch_all(&mut *tx)
    .await?;
    let rows = component_rows(&mut tx, page, internal).await?;
    let shown = Shown::new(&rows);
    let incidents: Vec<IncidentRow> = sqlx::query_as(concat!(
        select_incident!(),
        " WHERE p.page_id = ? AND (i.resolved_at IS NULL OR i.resolved_at >= ?)
          ORDER BY i.started_at DESC, i.id DESC LIMIT ?"
    ))
    .bind(page.id)
    .bind(window_start)
    .bind(INCIDENTS_MAX)
    .fetch_all(&mut *tx)
    .await?;
    let published = published(&mut tx, incidents, &shown, locale).await?;

    let monitors: Vec<MonitorId> = rows
        .iter()
        .filter_map(|row| row.monitor_id.map(MonitorId::new))
        .collect();
    let shown_maintenance =
        maintenance::shown_for(&mut tx, &monitors, now, now + MAINTENANCE_AHEAD_MS).await?;
    let probes: Vec<(i64, String, i64, Option<i64>, i64)> = if internal {
        sqlx::query_as(
            "SELECT s.monitor_id, p.name, s.status, s.latency_ms, s.checked_at
             FROM probe_state s JOIN probes p ON p.id = s.probe_id
             JOIN monitor_probes mp ON mp.monitor_id = s.monitor_id AND mp.probe_id = s.probe_id
             JOIN page_components pc ON pc.monitor_id = s.monitor_id AND pc.page_id = ?
             ORDER BY p.id",
        )
        .bind(page.id)
        .fetch_all(&mut *tx)
        .await?
    } else {
        Vec::new()
    };
    tx.rollback().await?;

    let storage_ids: Vec<storage::MonitorId> = monitors
        .iter()
        .map(|monitor| storage::MonitorId(monitor.get()))
        .collect();
    let mut day_bars = state
        .storage
        .day_bars(&storage_ids, Series::Monitor, zone, today, DAY_BARS_MAX)
        .await
        .map_err(read_failed)?
        .value;
    let mut latency: HashMap<i64, Vec<LatencyPoint>> = HashMap::new();
    if internal {
        for monitor in &storage_ids {
            let points = recent_latency(state, *monitor, now).await?;
            latency.insert(
                monitor.0,
                points
                    .into_iter()
                    .map(|point| LatencyPoint {
                        bucket: point.bucket_ms,
                        min_ms: point.latency.min_ms,
                        max_ms: point.latency.max_ms,
                        avg_ms: point.latency.avg_ms,
                    })
                    .collect(),
            );
        }
    }

    let days: Vec<NaiveDate> = first.iter_days().take(DAY_BARS_MAX as usize).collect();
    let mut components: BTreeMap<i64, Vec<PublicComponent>> = BTreeMap::new();
    for row in &rows {
        let id = PageComponentId::new(row.id);
        let has_monitor = row.monitor_id.is_some();
        let open_incident = published
            .iter()
            .filter(|p| p.incident.resolved_at.is_none())
            .filter_map(|p| p.current.get(&id))
            .copied()
            .max();
        let monitor_status = match row.active {
            Some(true) => row.status.and_then(monitor_color),
            _ => None,
        };
        let in_maintenance = row.in_maintenance || monitor_status == Some(PageStatus::Maintenance);
        let status = component_status(monitor_status, has_monitor, in_maintenance, open_incident);

        let recorded: HashMap<NaiveDate, storage::DayBar> = row
            .monitor_id
            .and_then(|monitor| day_bars.remove(&storage::MonitorId(monitor)))
            .unwrap_or_default()
            .into_iter()
            .map(|bar| (bar.day, bar))
            .collect();
        let created = row.created_at.map(|at| local_day(at, zone));
        let mut total = Counts::default();
        let bars = days
            .iter()
            .map(|day| {
                let bar = recorded.get(day);
                let counts = bar.map(|bar| bar.counts).unwrap_or_default();
                total.up += counts.up;
                total.down += counts.down;
                total.pending += counts.pending;
                let touching: Vec<&Published> = published
                    .iter()
                    .filter(|p| p.incident.components.contains(&id))
                    .filter(|p| {
                        let start = local_day(p.incident.started_at, zone);
                        let end = local_day(p.incident.resolved_at.unwrap_or(now), zone);
                        (start..=end).contains(day)
                    })
                    .collect();
                let uptime = uptime_of(&counts);
                let maintenance = counts.maintenance > 0;
                PageBar {
                    day: day.to_string(),
                    status: bar_status(&Day {
                        uptime,
                        maintenance,
                        incident: touching
                            .iter()
                            .filter_map(|p| p.peak.get(&id))
                            .copied()
                            .max(),
                        has_monitor,
                        existed: created.is_none_or(|created| created <= *day),
                    }),
                    uptime,
                    utc_approximate: bar.is_some_and(|bar| bar.utc_approximate),
                    maintenance,
                    incidents: touching.iter().map(|p| p.incident.id).collect(),
                }
            })
            .collect();
        let detail = match (internal, row.monitor_id) {
            (true, Some(monitor)) => Some(ComponentDetail {
                latency: latency.remove(&monitor).unwrap_or_default(),
                probes: probes
                    .iter()
                    .filter(|probe| probe.0 == monitor)
                    .filter_map(|(_, name, status, latency_ms, checked_at)| {
                        Some(ProbeView {
                            name: name.clone(),
                            status: wire_status(status_from_code(*status)?),
                            latency_ms: latency_ms.map(|ms| ms as u32),
                            checked_at: *checked_at,
                        })
                    })
                    .collect(),
            }),
            _ => None,
        };
        components
            .entry(row.group_id)
            .or_default()
            .push(PublicComponent {
                id,
                name: row.name.clone(),
                description: row.description.clone(),
                status,
                uptime_90d: has_monitor.then(|| uptime_of(&total)).flatten(),
                bars,
                detail,
            });
    }

    let status = components
        .values()
        .flatten()
        .filter_map(|component| component.status)
        .max()
        .unwrap_or(PageStatus::Operational);
    let groups = groups
        .into_iter()
        .filter_map(|(id, name, default_open)| {
            let components = components.remove(&id)?;
            Some(PublicGroup {
                id: PageGroupId::new(id),
                name,
                default_open,
                components,
            })
        })
        .collect();
    let maintenance = shown_maintenance
        .into_iter()
        .filter_map(|window| {
            let components: Vec<PageComponentId> = window
                .monitors
                .iter()
                .filter_map(|monitor| shown.by_monitor.get(&monitor.get()).copied())
                .collect();
            (!components.is_empty()).then_some(PublicMaintenance {
                title: window.title,
                description: window.description,
                active: window.active,
                starts_at: window.window.start,
                ends_at: window.window.end,
                components,
            })
        })
        .collect();

    Ok(PageSnapshot {
        appearance: appearance(page),
        description: page.description.clone(),
        footer: page.footer.clone(),
        timezone: page.timezone.clone(),
        locale,
        audience,
        status,
        groups,
        incidents: published.into_iter().map(|p| p.incident).collect(),
        maintenance,
        cursor: Topic::page(page.page()).cursor(head),
        generated_at: now,
    })
}

/// Where an incident history page ends: the last incident's start and ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryCursor {
    started_at: i64,
    id: i64,
}

impl HistoryCursor {
    /// `<started_at>.<id>`, as [`HistoryCursor::to_string`] writes it.
    pub fn parse(text: &str) -> Option<Self> {
        let (started_at, id) = text.split_once('.')?;
        Some(Self {
            started_at: started_at.parse().ok()?,
            id: id.parse().ok()?,
        })
    }
}

impl std::fmt::Display for HistoryCursor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.started_at, self.id)
    }
}

/// Up to `limit` incidents published to `page` that name a component `audience` sees, newest
/// first, after `after`. Unlike the snapshot's list it reaches back without a time bound.
pub(super) async fn incident_history(
    state: &AppState,
    page: &PageRow,
    audience: PageAudience,
    locale: Locale,
    after: Option<HistoryCursor>,
    limit: u32,
) -> Result<PageIncidents, ApiError> {
    let internal = audience == PageAudience::Internal;
    let after = after.unwrap_or(HistoryCursor {
        started_at: i64::MAX,
        id: i64::MAX,
    });
    let mut tx = state.db.reader.begin().await?;
    let rows = component_rows(&mut tx, page, internal).await?;
    let shown = Shown::new(&rows);
    let mut incidents: Vec<IncidentRow> = sqlx::query_as(concat!(
        select_incident!(),
        " WHERE p.page_id = ?1 AND (i.started_at, i.id) < (?2, ?3)
            AND EXISTS (SELECT 1 FROM incident_targets t
                        JOIN page_components pc ON pc.page_id = ?1 AND (?4 OR pc.internal = 0)
                          AND (pc.monitor_id = t.monitor_id OR pc.component_id = t.component_id)
                        WHERE t.incident_id = i.id)
          ORDER BY i.started_at DESC, i.id DESC LIMIT ?5"
    ))
    .bind(page.id)
    .bind(after.started_at)
    .bind(after.id)
    .bind(internal)
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut *tx)
    .await?;
    let more = incidents.len() > limit as usize;
    incidents.truncate(limit as usize);
    let next_cursor = more.then(|| incidents.last()).flatten().map(|row| {
        HistoryCursor {
            started_at: row.started_at,
            id: row.id,
        }
        .to_string()
    });
    let published = published(&mut tx, incidents, &shown, locale).await?;
    tx.rollback().await?;
    Ok(PageIncidents {
        incidents: published.into_iter().map(|p| p.incident).collect(),
        next_cursor,
    })
}

/// Response times of the last 24 hours per hour. A monitor with fewer than two hourly points,
/// such as a new one, gets the last two hours per minute instead, so its chart fills within
/// minutes rather than after an hour.
async fn recent_latency(
    state: &AppState,
    monitor: storage::MonitorId,
    now: i64,
) -> Result<Vec<storage::LatencyPoint>, ApiError> {
    let read = |resolution: Resolution, span: i64| async move {
        let step = resolution.step_ms();
        let to = now.div_euclid(step) * step + step;
        state
            .storage
            .latency(monitor, Series::Monitor, resolution, to - span, to)
            .await
            .map(|read| read.value)
            .map_err(read_failed)
    };
    let hourly = read(Resolution::Hour, DAY_MS).await?;
    if hourly.len() >= 2 {
        return Ok(hourly);
    }
    let minutes = read(Resolution::Minute, RECENT_MS).await?;
    Ok(if minutes.len() > hourly.len() {
        minutes
    } else {
        hourly
    })
}

/// Uptime of the page's visible monitors, or of one component, over a badge window.
pub(super) async fn uptime(
    state: &AppState,
    monitors: &[MonitorId],
    window: storage::UptimeWindow,
) -> Result<Option<f64>, ApiError> {
    let ids: Vec<storage::MonitorId> = monitors
        .iter()
        .map(|monitor| storage::MonitorId(monitor.get()))
        .collect();
    let counts = state
        .storage
        .uptime(&ids, Series::Monitor, window, state.clock.now_ms())
        .await
        .map_err(read_failed)?
        .value;
    let mut total = Counts::default();
    for counts in counts.values() {
        total.up += counts.up;
        total.down += counts.down;
        total.pending += counts.pending;
    }
    Ok(uptime_of(&total))
}

/// Uptime of `monitors` over the page's 90 daily bars.
pub(super) async fn uptime_90d(
    state: &AppState,
    page: &PageRow,
    monitors: &[MonitorId],
) -> Result<Option<f64>, ApiError> {
    let ids: Vec<storage::MonitorId> = monitors
        .iter()
        .map(|monitor| storage::MonitorId(monitor.get()))
        .collect();
    let zone = page.zone();
    let today = local_day(state.clock.now_ms(), zone);
    let bars = state
        .storage
        .day_bars(&ids, Series::Monitor, zone, today, DAY_BARS_MAX)
        .await
        .map_err(read_failed)?
        .value;
    let mut total = Counts::default();
    for bar in bars.values().flatten() {
        total.up += bar.counts.up;
        total.down += bar.counts.down;
        total.pending += bar.counts.pending;
    }
    Ok(uptime_of(&total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(uptime: Option<f64>, maintenance: bool, incident: Option<Impact>) -> Option<PageStatus> {
        bar_status(&Day {
            uptime,
            maintenance,
            incident,
            has_monitor: true,
            existed: true,
        })
    }

    #[test]
    fn bars_follow_uptime_thresholds_unless_an_incident_was_published() {
        assert_eq!(day(Some(1.0), false, None), Some(PageStatus::Operational));
        assert_eq!(day(Some(0.995), false, None), Some(PageStatus::Degraded));
        assert_eq!(
            day(Some(0.97), false, None),
            Some(PageStatus::PartialOutage)
        );
        assert_eq!(day(Some(0.5), false, None), Some(PageStatus::MajorOutage));
        assert_eq!(day(None, false, None), None);
        assert_eq!(day(None, true, None), Some(PageStatus::Maintenance));
        assert_eq!(day(Some(1.0), true, None), Some(PageStatus::Maintenance));
        assert_eq!(
            day(Some(0.5), false, Some(Impact::Degraded)),
            Some(PageStatus::Degraded)
        );
        assert_eq!(
            day(Some(0.5), false, Some(Impact::Operational)),
            Some(PageStatus::MajorOutage)
        );
        let component = |existed, incident| {
            bar_status(&Day {
                uptime: None,
                maintenance: false,
                incident,
                has_monitor: false,
                existed,
            })
        };
        assert_eq!(component(true, None), Some(PageStatus::Operational));
        assert_eq!(component(false, None), None);
        assert_eq!(
            component(false, Some(Impact::Degraded)),
            Some(PageStatus::Degraded)
        );
    }

    #[test]
    fn maintenance_overrides_the_monitor_but_not_a_published_incident() {
        let down = Some(PageStatus::MajorOutage);
        assert_eq!(
            component_status(down, true, true, None),
            Some(PageStatus::Maintenance)
        );
        assert_eq!(
            component_status(down, true, true, Some(Impact::PartialOutage)),
            Some(PageStatus::PartialOutage)
        );
        assert_eq!(
            component_status(
                Some(PageStatus::Operational),
                true,
                false,
                Some(Impact::Degraded)
            ),
            Some(PageStatus::Degraded)
        );
        assert_eq!(
            component_status(None, false, false, None),
            Some(PageStatus::Operational)
        );
        assert_eq!(component_status(None, true, false, None), None);
    }

    #[test]
    fn local_days_drop_offset_minutes() {
        let zone: Tz = "Asia/Kolkata".parse().unwrap();
        // 18:45 UTC is 00:15 at +05:30 but 23:45 at +05:00.
        let ms = chrono::NaiveDate::from_ymd_opt(2026, 3, 1)
            .unwrap()
            .and_hms_opt(18, 45, 0)
            .unwrap()
            .and_utc()
            .timestamp_millis();
        assert_eq!(local_day(ms, zone).to_string(), "2026-03-01");
    }
}
