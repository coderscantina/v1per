//! `<base>/response-times.json`: one monitor component's response times over a range, per
//! probe or probe location, with percentiles and the incidents and maintenance that touched
//! it (`docs/contracts/status-page-api.md`).
//!
//! The audience rules follow the snapshot's: the `internal` audience gets a series per
//! assigned probe, named by the probe; the `public` one gets response times only on a page with
//! `response_times`, and series per location (city and country, never a probe's name) only
//! with `probe_locations` too. Marks reuse the snapshot's incident and maintenance builders.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use v1per_protocol::envelope::Topic;
use v1per_protocol::ids::{MonitorId, PageComponentId};
use v1per_protocol::operations::monitors::LatencyPoint;
use v1per_protocol::status_page::{
    Locale, PageAudience, PageResponseTimes, ResponseMark, ResponseMarkKind, ResponseRange,
    ResponseSeries,
};
use v1per_storage::{self as storage, ProbeId, Resolution};

use super::PageRow;
use super::snapshot::{
    INCIDENTS_MAX, IncidentRow, Shown, component_rows, published, read_failed, select_incident,
};
use crate::error::ApiError;
use crate::events;
use crate::incidents::maintenance;
use crate::state::AppState;

const MINUTE_MS: i64 = 60_000;
const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;
/// The span of per-minute points for a monitor without two hourly points yet, as the snapshot.
const RECENT_MS: i64 = 2 * HOUR_MS;
/// An answer is rebuilt after this long even when the page did not change.
const CACHE_TTL_MS: i64 = 60_000;
/// Above this many entries the cache starts over.
const CACHE_MAX: usize = 1024;

/// The rollup a range reads and the bucket width of its points: at most 180 points each.
fn plan(range: ResponseRange) -> (Resolution, i64, i64) {
    match range {
        ResponseRange::Day => (Resolution::Hour, HOUR_MS, DAY_MS),
        ResponseRange::Week => (Resolution::Hour, HOUR_MS, 7 * DAY_MS),
        ResponseRange::Month => (Resolution::Hour, 4 * HOUR_MS, 30 * DAY_MS),
        ResponseRange::Quarter => (Resolution::Day, DAY_MS, 90 * DAY_MS),
    }
}

type Key = (i64, PageAudience, Locale, i64, ResponseRange);

struct Entry {
    head: i64,
    built_at: i64,
    json: Arc<str>,
}

/// Built answers per page, audience, locale, component and range, valid while the page's topic
/// stays at the same head, for at most a minute. Unlike the snapshot cache it does not follow
/// the heartbeat watermark: on a busy instance that moves every second, a new beat changes only
/// the newest point, and the percentiles scan raw heartbeats.
#[derive(Default)]
pub struct ResponseTimesCache(Mutex<HashMap<Key, Entry>>);

/// The answer for `component` over `range`, from the cache when still current. `404` when the
/// audience gets no response times for it.
pub(super) async fn cached(
    state: &AppState,
    page: &PageRow,
    audience: PageAudience,
    locale: Locale,
    component: PageComponentId,
    range: ResponseRange,
) -> Result<Arc<str>, ApiError> {
    if audience == PageAudience::Public && !page.response_times {
        return Err(ApiError::not_found());
    }
    let now = state.clock.now_ms();
    let head = {
        let mut conn = state.db.reader.acquire().await?;
        events::head(&mut conn, &Topic::page(page.page())).await?
    };
    let key = (page.id, audience, locale, component.get(), range);
    {
        let cache = state
            .page_response_times
            .0
            .lock()
            .expect("response times cache");
        if let Some(entry) = cache.get(&key)
            && entry.head == head
            && now - entry.built_at < CACHE_TTL_MS
        {
            return Ok(entry.json.clone());
        }
    }
    let answer = build(state, page, audience, locale, component, range, now).await?;
    let json: Arc<str> = serde_json::to_string(&answer)
        .map_err(ApiError::internal)?
        .into();
    let mut cache = state
        .page_response_times
        .0
        .lock()
        .expect("response times cache");
    if cache.len() >= CACHE_MAX {
        cache.clear();
    }
    cache.insert(
        key,
        Entry {
            head,
            built_at: now,
            json: json.clone(),
        },
    );
    Ok(json)
}

/// "Vienna, AT", "AT" or "Vienna"; `None` without either.
fn location(city: Option<&str>, country: Option<&str>) -> Option<String> {
    match (city, country) {
        (Some(city), Some(country)) => Some(format!("{city}, {country}")),
        (city, country) => city.or(country).map(str::to_owned),
    }
}

/// A series the audience sees: its label, country and the probes it merges.
struct Group {
    label: String,
    country: Option<String>,
    probes: Vec<ProbeId>,
}

#[derive(sqlx::FromRow)]
struct ProbeRow {
    id: i64,
    name: String,
    country: Option<String>,
    city: Option<String>,
}

/// One series per probe for the `internal` audience; per city and country for the `public`
/// one, probes without a country left out.
fn groups(probes: Vec<ProbeRow>, internal: bool) -> Vec<Group> {
    if internal {
        return probes
            .into_iter()
            .map(|probe| Group {
                label: match location(probe.city.as_deref(), probe.country.as_deref()) {
                    Some(place) => format!("{} ({place})", probe.name),
                    None => probe.name,
                },
                country: probe.country,
                probes: vec![ProbeId(probe.id)],
            })
            .collect();
    }
    let mut places: BTreeMap<(String, Option<String>), Vec<ProbeId>> = BTreeMap::new();
    for probe in probes {
        if let Some(country) = probe.country {
            places
                .entry((country, probe.city))
                .or_default()
                .push(ProbeId(probe.id));
        }
    }
    let mut groups: Vec<Group> = places
        .into_iter()
        .map(|((country, city), probes)| Group {
            label: location(city.as_deref(), Some(&country)).unwrap_or_default(),
            country: Some(country),
            probes,
        })
        .collect();
    groups.sort_by(|a, b| a.label.cmp(&b.label));
    groups
}

fn points(points: Vec<storage::LatencyPoint>) -> Vec<LatencyPoint> {
    points
        .into_iter()
        .map(|point| LatencyPoint {
            bucket: point.bucket_ms,
            min_ms: point.latency.min_ms,
            max_ms: point.latency.max_ms,
            avg_ms: point.latency.avg_ms,
        })
        .collect()
}

fn series(latency: storage::SeriesLatency, group: Option<Group>) -> ResponseSeries {
    let (label, country) = group.map_or((None, None), |group| (Some(group.label), group.country));
    let summary = latency.summary;
    ResponseSeries {
        label,
        country,
        points: points(latency.points),
        avg_ms: summary.map(|s| s.avg_ms),
        p50_ms: summary.map(|s| s.p50_ms),
        p95_ms: summary.map(|s| s.p95_ms),
        p99_ms: summary.map(|s| s.p99_ms),
    }
}

async fn build(
    state: &AppState,
    page: &PageRow,
    audience: PageAudience,
    locale: Locale,
    component: PageComponentId,
    range: ResponseRange,
    now: i64,
) -> Result<PageResponseTimes, ApiError> {
    let internal = audience == PageAudience::Internal;
    let (resolution, step, span) = plan(range);
    let to = now.div_euclid(step) * step + step;
    let from = to - span;

    let mut tx = state.db.reader.begin().await?;
    let rows = component_rows(&mut tx, page, internal).await?;
    let monitor = rows
        .iter()
        .find(|row| row.id == component.get())
        .and_then(|row| row.monitor_id)
        .ok_or_else(ApiError::not_found)?;
    let shown = Shown::new(&rows);
    let groups = if internal || page.probe_locations {
        let probes: Vec<ProbeRow> = sqlx::query_as(
            "SELECT p.id, p.name, p.country, p.city
             FROM monitor_probes mp JOIN probes p ON p.id = mp.probe_id
             WHERE mp.monitor_id = ? ORDER BY p.id",
        )
        .bind(monitor)
        .fetch_all(&mut *tx)
        .await?;
        groups(probes, internal)
    } else {
        Vec::new()
    };
    let incidents: Vec<IncidentRow> = sqlx::query_as(concat!(
        select_incident!(),
        " WHERE p.page_id = ? AND i.started_at < ? AND (i.resolved_at IS NULL OR i.resolved_at >= ?)
          ORDER BY i.started_at DESC, i.id DESC LIMIT ?"
    ))
    .bind(page.id)
    .bind(to)
    .bind(from)
    .bind(INCIDENTS_MAX)
    .fetch_all(&mut *tx)
    .await?;
    let published = published(&mut tx, incidents, &shown, locale).await?;
    let windows = maintenance::windows_between(&mut tx, MonitorId::new(monitor), from, now).await?;
    tx.rollback().await?;

    let read = |resolution: Resolution, step: i64, from: i64| {
        let probes: Vec<Vec<ProbeId>> = groups.iter().map(|group| group.probes.clone()).collect();
        async move {
            state
                .storage
                .response_times(
                    storage::MonitorId(monitor),
                    probes,
                    resolution,
                    step,
                    from,
                    to,
                )
                .await
                .map(|read| read.value)
                .map_err(read_failed)
        }
    };
    let (mut latency, mut from, mut step) = (read(resolution, step, from).await?, from, step);
    if range == ResponseRange::Day && latency.overall.points.len() < 2 {
        let minutes = read(Resolution::Minute, MINUTE_MS, to - RECENT_MS).await?;
        if minutes.overall.points.len() > latency.overall.points.len() {
            (latency, from, step) = (minutes, to - RECENT_MS, MINUTE_MS);
        }
    }

    let id = component;
    let mut marks: Vec<ResponseMark> = published
        .into_iter()
        .filter(|p| p.incident.components.contains(&id))
        .filter(|p| p.incident.resolved_at.is_none_or(|at| at >= from))
        .map(|p| ResponseMark {
            kind: ResponseMarkKind::Incident,
            incident: Some(p.incident.id),
            impact: p.peak.get(&id).copied(),
            title: p.incident.title,
            from: p.incident.started_at,
            to: p.incident.resolved_at,
        })
        .chain(
            windows
                .into_iter()
                .filter(|window| window.end.is_none_or(|end| end >= from))
                .map(|window| ResponseMark {
                    kind: ResponseMarkKind::Maintenance,
                    incident: None,
                    impact: None,
                    title: window.title,
                    from: window.start,
                    to: window.end,
                }),
        )
        .collect();
    marks.sort_by_key(|mark| mark.from);

    Ok(PageResponseTimes {
        range,
        from,
        to,
        step,
        overall: series(latency.overall, None),
        series: latency
            .groups
            .into_iter()
            .zip(groups)
            .map(|(latency, group)| series(latency, Some(group)))
            .collect(),
        marks,
    })
}
