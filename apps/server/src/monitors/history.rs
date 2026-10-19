//! Reads of heartbeat storage for one monitor's history: uptime windows, latency, daily bars
//! and the raw heartbeat list, of the monitor or of one probe's own results. Each runs in its
//! own DuckDB snapshot, separate from the control database's.

use std::collections::BTreeMap;

use chrono::{DateTime, Days, NaiveDate};
use chrono_tz::Tz;
use v1per_protocol::errors::{ErrorCode, FieldErrorCode};
use v1per_protocol::ids::{MonitorId, ProbeId};
use v1per_protocol::limits::{
    DAY_BARS_MAX, HEARTBEATS_PAGE_DEFAULT, HEARTBEATS_PAGE_MAX, LATENCY_POINTS_MAX,
};
use v1per_protocol::operations::monitors::{
    DayBar, DayBarList, DayBarsArgs, Heartbeat, HeartbeatPage, HeartbeatsArgs, HourPoint,
    HourState, LatencyArgs, LatencyPoint, LatencySeries, MonitorHourly, MonitorHourlyList,
    MonitorUptime, Resolution, ScopeTrend, TrendPoint, UptimeArgs,
};
use v1per_storage::{
    self as storage, Counts, HeartbeatCursor, ReadError, Series, Storage, UptimeWindow,
};

use super::wire_status;
use crate::error::{ApiError, FieldErrors};

fn read_failed(error: ReadError) -> ApiError {
    match error {
        ReadError::Busy | ReadError::Timeout => ApiError::new(ErrorCode::QueryTimeout),
        other => ApiError::internal(other),
    }
}

fn id(monitor: MonitorId) -> storage::MonitorId {
    storage::MonitorId(monitor.get())
}

fn probe_id(probe: ProbeId) -> storage::ProbeId {
    storage::ProbeId(probe.get())
}

fn series(probe: Option<ProbeId>) -> Series {
    probe.map_or(Series::Monitor, |probe| Series::Probe(probe_id(probe)))
}

/// 24-hour uptime of each monitor's monitor-level series.
pub async fn uptime_24h(
    storage: &Storage,
    monitors: &[MonitorId],
    now_ms: i64,
) -> Result<BTreeMap<MonitorId, f64>, ApiError> {
    let ids: Vec<_> = monitors.iter().copied().map(id).collect();
    let counts = storage
        .uptime(&ids, Series::Monitor, UptimeWindow::Day, now_ms)
        .await
        .map_err(read_failed)?
        .value;
    Ok(counts
        .into_iter()
        .filter_map(|(monitor, counts)| Some((MonitorId::new(monitor.0), counts.uptime()?)))
        .collect())
}

/// The last 24 UTC hours of the monitors taken together, the current hour last.
pub async fn trend(
    storage: &Storage,
    monitors: &[MonitorId],
    now_ms: i64,
) -> Result<ScopeTrend, ApiError> {
    let ids: Vec<_> = monitors.iter().copied().map(id).collect();
    let buckets = storage
        .trend(&ids, now_ms, 24)
        .await
        .map_err(read_failed)?
        .value;
    Ok(ScopeTrend {
        points: buckets
            .into_iter()
            .map(|bucket| TrendPoint {
                bucket: bucket.bucket_ms,
                uptime: bucket.counts.uptime(),
                latency_ms: bucket.counts.latency.map(|latency| latency.avg_ms),
                healthy: bucket.healthy,
                down: bucket.down,
                pending: bucket.pending,
                maintenance: bucket.maintenance,
            })
            .collect(),
    })
}

/// The last 24 UTC hours of each monitor, the current hour last.
pub async fn hourly(
    storage: &Storage,
    monitors: &[MonitorId],
    now_ms: i64,
) -> Result<MonitorHourlyList, ApiError> {
    let ids: Vec<_> = monitors.iter().copied().map(id).collect();
    let hourly = storage
        .hourly(&ids, now_ms, 24)
        .await
        .map_err(read_failed)?
        .value;
    Ok(MonitorHourlyList {
        from: hourly.from_ms,
        monitors: hourly
            .counts
            .into_iter()
            .map(|(monitor, hours)| MonitorHourly {
                monitor: MonitorId::new(monitor.0),
                points: hours.iter().map(hour_point).collect(),
            })
            .collect(),
    })
}

fn hour_point(counts: &Counts) -> HourPoint {
    let state = if counts.down > 0 {
        Some(HourState::Down)
    } else if counts.pending > 0 {
        Some(HourState::Pending)
    } else if counts.maintenance > 0 {
        Some(HourState::Maintenance)
    } else if counts.up > 0 {
        Some(HourState::Up)
    } else {
        None
    };
    HourPoint {
        state,
        uptime: counts.uptime(),
        latency_ms: counts.latency.map(|latency| latency.avg_ms),
    }
}

pub async fn uptime(
    storage: &Storage,
    args: &UptimeArgs,
    now_ms: i64,
) -> Result<MonitorUptime, ApiError> {
    let (monitor, series) = (args.monitor, series(args.probe));
    let window = |window| async move {
        let counts = storage
            .uptime(&[id(monitor)], series, window, now_ms)
            .await
            .map_err(read_failed)?
            .value;
        Ok::<_, ApiError>(counts.get(&id(monitor)).and_then(Counts::uptime))
    };
    let (day, week, month, year) = tokio::join!(
        window(UptimeWindow::Day),
        window(UptimeWindow::Week),
        window(UptimeWindow::Month),
        window(UptimeWindow::Year),
    );
    Ok(MonitorUptime {
        uptime_24h: day?,
        uptime_7d: week?,
        uptime_30d: month?,
        uptime_1y: year?,
    })
}

pub async fn latency(storage: &Storage, args: &LatencyArgs) -> Result<LatencySeries, ApiError> {
    let resolution = match args.resolution {
        Resolution::Minute => storage::Resolution::Minute,
        Resolution::Hour => storage::Resolution::Hour,
        Resolution::Day => storage::Resolution::Day,
    };
    let buckets = (args.to - args.from) / resolution.step_ms();
    if args.from >= args.to || buckets > i64::from(LATENCY_POINTS_MAX) {
        return Err(FieldErrors::one(
            "to",
            FieldErrorCode::InvalidValue,
            Some(LATENCY_POINTS_MAX),
        ));
    }
    let points = storage
        .latency(
            id(args.monitor),
            series(args.probe),
            resolution,
            args.from,
            args.to,
        )
        .await
        .map_err(read_failed)?
        .value;
    Ok(LatencySeries {
        points: points
            .into_iter()
            .map(|point| LatencyPoint {
                bucket: point.bucket_ms,
                min_ms: point.latency.min_ms,
                max_ms: point.latency.max_ms,
                avg_ms: point.latency.avg_ms,
            })
            .collect(),
    })
}

/// One bar per day, today included; days without beats have zero counts.
pub async fn day_bars(
    storage: &Storage,
    args: &DayBarsArgs,
    now_ms: i64,
) -> Result<DayBarList, ApiError> {
    let days = args.days.unwrap_or(DAY_BARS_MAX);
    let mut errors = FieldErrors::default();
    if !(1..=DAY_BARS_MAX).contains(&days) {
        errors.add("days", FieldErrorCode::InvalidValue, Some(DAY_BARS_MAX));
    }
    let timezone = args.timezone.as_deref().unwrap_or("UTC");
    let zone: Option<Tz> = timezone.parse().ok();
    if zone.is_none() {
        errors.add("timezone", FieldErrorCode::InvalidValue, None);
    }
    errors.finish()?;
    let zone = zone.expect("checked above");
    let today = DateTime::from_timestamp_millis(now_ms)
        .ok_or_else(|| ApiError::internal("clock out of range"))?
        .with_timezone(&zone)
        .date_naive();
    let mut bars = storage
        .day_bars(&[id(args.monitor)], series(args.probe), zone, today, days)
        .await
        .map_err(read_failed)?
        .value;
    let recorded: BTreeMap<NaiveDate, storage::DayBar> = bars
        .remove(&id(args.monitor))
        .unwrap_or_default()
        .into_iter()
        .map(|bar| (bar.day, bar))
        .collect();
    let first = today
        .checked_sub_days(Days::new(u64::from(days - 1)))
        .ok_or_else(|| ApiError::internal("date out of range"))?;
    Ok(DayBarList {
        timezone: timezone.to_owned(),
        bars: first
            .iter_days()
            .take(days as usize)
            .map(|day| {
                let bar = recorded.get(&day);
                let counts = bar.map(|bar| bar.counts).unwrap_or_default();
                DayBar {
                    day: day.to_string(),
                    up: counts.up as u32,
                    down: counts.down as u32,
                    pending: counts.pending as u32,
                    maintenance: counts.maintenance as u32,
                    uptime: counts.uptime(),
                    utc_approximate: bar.is_some_and(|bar| bar.utc_approximate),
                }
            })
            .collect(),
    })
}

/// Raw heartbeats, newest first. Visible once storage has moved them from its inbox, about a
/// second after admission.
pub async fn heartbeats(
    storage: &Storage,
    args: &HeartbeatsArgs,
) -> Result<HeartbeatPage, ApiError> {
    let mut errors = FieldErrors::default();
    let limit = args.limit.unwrap_or(HEARTBEATS_PAGE_DEFAULT);
    if !(1..=HEARTBEATS_PAGE_MAX).contains(&limit) {
        errors.add(
            "limit",
            FieldErrorCode::InvalidValue,
            Some(HEARTBEATS_PAGE_MAX),
        );
    }
    let after = args.cursor.as_deref().map(parse_cursor);
    if matches!(after, Some(None)) {
        errors.add("cursor", FieldErrorCode::InvalidValue, None);
    }
    errors.finish()?;
    let page = storage
        .heartbeats(
            id(args.monitor),
            args.probe.map(probe_id),
            limit,
            after.flatten(),
        )
        .await
        .map_err(read_failed)?
        .value;
    Ok(HeartbeatPage {
        heartbeats: page
            .heartbeats
            .into_iter()
            .map(|beat| Heartbeat {
                time: beat.time_ms,
                probe: beat.probe.map(|probe| ProbeId::new(probe.0)),
                status: wire_status(beat.status),
                latency_ms: beat.latency_ms,
                message: beat.message,
                retries: beat.retries,
                http_status: beat.http_status,
                cert_expiry: beat.cert_expiry_ms,
            })
            .collect(),
        next_cursor: page.next.map(format_cursor),
    })
}

/// `<time>` or `<time>.<probe>`: where a page ended.
fn format_cursor(cursor: HeartbeatCursor) -> String {
    match cursor.probe {
        Some(probe) => format!("{}.{}", cursor.time_ms, probe.0),
        None => cursor.time_ms.to_string(),
    }
}

fn parse_cursor(text: &str) -> Option<HeartbeatCursor> {
    let (time, probe) = match text.split_once('.') {
        Some((time, probe)) => (time, Some(storage::ProbeId(probe.parse().ok()?))),
        None => (text, None),
    };
    Some(HeartbeatCursor {
        time_ms: time.parse().ok()?,
        probe,
    })
}
