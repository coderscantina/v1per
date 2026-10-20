//! Bounded typed reads over rollups (monitoring contract, Uptime), and the latest raw row.
//!
//! Rollups are kept per (monitor, probe); probe NULL is the monitor-level series. Reads of
//! that series combine native rollups with imported Kuma `stat_*` rows of the same
//! resolution. Per monitor, Kuma rows cover everything up to the end of its last imported
//! bucket and native rows count only from there on, so uptime for imported periods matches
//! Kuma.

use std::collections::BTreeMap;

use chrono::{DateTime, Days, NaiveDate, Offset};
use chrono_tz::Tz;
use duckdb::Connection;
use duckdb::types::Value;

use crate::heartbeats::ts;
use crate::record::{Heartbeat, MonitorId, ProbeId, Status};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 86_400_000;

/// Rollup bucket width. Buckets are aligned in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Minute,
    Hour,
    Day,
}

impl Resolution {
    pub const ALL: [Resolution; 3] = [Resolution::Minute, Resolution::Hour, Resolution::Day];

    pub fn step_ms(self) -> i64 {
        match self {
            Resolution::Minute => 60_000,
            Resolution::Hour => HOUR_MS,
            Resolution::Day => DAY_MS,
        }
    }

    pub(crate) fn table(self) -> &'static str {
        match self {
            Resolution::Minute => "rollup_minute",
            Resolution::Hour => "rollup_hour",
            Resolution::Day => "rollup_day",
        }
    }

    pub(crate) fn unit(self) -> &'static str {
        match self {
            Resolution::Minute => "minute",
            Resolution::Hour => "hour",
            Resolution::Day => "day",
        }
    }

    /// `kuma_stats.resolution`: Kuma's `stat_minutely`, `stat_hourly`, `stat_daily`.
    pub(crate) fn kuma_code(self) -> u8 {
        match self {
            Resolution::Minute => 1,
            Resolution::Hour => 2,
            Resolution::Day => 3,
        }
    }
}

/// The uptime periods Kuma shows, each over Kuma's buckets and including the current one
/// (decision 0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UptimeWindow {
    /// 1440 minute buckets.
    Day,
    /// 168 hour buckets.
    Week,
    /// 30 UTC days.
    Month,
    /// 365 UTC days.
    Year,
}

impl UptimeWindow {
    fn resolution_and_span(self) -> (Resolution, i64) {
        match self {
            UptimeWindow::Day => (Resolution::Minute, DAY_MS),
            UptimeWindow::Week => (Resolution::Hour, 7 * DAY_MS),
            UptimeWindow::Month => (Resolution::Day, 30 * DAY_MS),
            UptimeWindow::Year => (Resolution::Day, 365 * DAY_MS),
        }
    }
}

/// Heartbeat counts over a span, with the latency of its up heartbeats.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Counts {
    pub up: u64,
    pub down: u64,
    pub pending: u64,
    pub maintenance: u64,
    pub latency: Option<Latency>,
}

impl Counts {
    /// `up / (up + down)` with pending counted as down and maintenance left out, as Kuma
    /// counts (decision 0008); `None` without any up or down heartbeat. Kuma rows already
    /// fold pending into `down` and carry a pending count of 0.
    pub fn uptime(&self) -> Option<f64> {
        let total = self.up + self.pending + self.down;
        (total > 0).then(|| self.up as f64 / total as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Latency {
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
}

/// One local day of a status page bar.
#[derive(Debug, Clone, PartialEq)]
pub struct DayBar {
    pub day: NaiveDate,
    pub counts: Counts,
    /// Taken from Kuma's daily row: the UTC day with the same date, not the local day.
    pub utc_approximate: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LatencyPoint {
    /// Bucket start, UTC Unix milliseconds.
    pub bucket_ms: i64,
    pub latency: Latency,
}

/// One UTC hour of a set of monitors' monitor-level series.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrendBucket {
    /// Bucket start, UTC Unix milliseconds.
    pub bucket_ms: i64,
    /// Every monitor's beats in the hour, summed.
    pub counts: Counts,
    /// Monitors without down or pending beats, maintenance ones included.
    pub healthy: u32,
    /// Monitors with at least one down beat.
    pub down: u32,
    /// Monitors with pending beats and no down beat.
    pub pending: u32,
    /// Monitors with maintenance beats and no down or pending beat.
    pub maintenance: u32,
}

/// Each monitor's last hours, by UTC hour.
#[derive(Debug, Clone, PartialEq)]
pub struct Hourly {
    /// Start of the first hour, UTC Unix milliseconds. Entry i starts at `from_ms` + i hours.
    pub from_ms: i64,
    /// Oldest first, the current partial hour last.
    pub counts: BTreeMap<MonitorId, Vec<Counts>>,
}

/// Rollup rows summed exactly: latency keeps sum and count so averages merge.
#[derive(Debug, Clone, Copy, Default)]
struct Sum {
    up: u64,
    down: u64,
    pending: u64,
    maintenance: u64,
    latency_min: Option<f64>,
    latency_max: Option<f64>,
    latency_sum: f64,
    latency_count: u64,
}

impl Sum {
    fn add(&mut self, other: &Sum) {
        self.up += other.up;
        self.down += other.down;
        self.pending += other.pending;
        self.maintenance += other.maintenance;
        self.latency_min = merge(self.latency_min, other.latency_min, f64::min);
        self.latency_max = merge(self.latency_max, other.latency_max, f64::max);
        self.latency_sum += other.latency_sum;
        self.latency_count += other.latency_count;
    }

    fn latency(&self) -> Option<Latency> {
        let (min_ms, max_ms) = (self.latency_min?, self.latency_max?);
        (self.latency_count > 0).then(|| Latency {
            min_ms,
            max_ms,
            avg_ms: self.latency_sum / self.latency_count as f64,
        })
    }

    fn counts(&self) -> Counts {
        Counts {
            up: self.up,
            down: self.down,
            pending: self.pending,
            maintenance: self.maintenance,
            latency: self.latency(),
        }
    }
}

fn merge(a: Option<f64>, b: Option<f64>, pick: fn(f64, f64) -> f64) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(pick(a, b)),
        (a, b) => a.or(b),
    }
}

fn id_list(monitors: &[MonitorId]) -> String {
    monitors
        .iter()
        .map(|m| m.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Which rollup series a read uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Series {
    /// The monitor-level series: evaluator entries, server-decided heartbeats and imported
    /// Kuma rows. What status pages show.
    Monitor,
    /// One probe's own results, for grant holders.
    Probe(ProbeId),
}

const NATIVE_COLUMNS: &str = "n.monitor_id, n.bucket, n.up, n.down, n.pending, n.maintenance,
                              n.latency_min::DOUBLE AS latency_min,
                              n.latency_max::DOUBLE AS latency_max,
                              n.latency_sum::DOUBLE AS latency_sum, n.latency_count";
const RANGE: &str = "n.bucket >= $1 AND n.bucket < $2";

/// SQL yielding the rollup rows of `series` (monitor, bucket, counts, latency) for the
/// monitor list `ids` with bucket in `[$1, $2)`. The monitor series adds imported Kuma rows
/// and drops native rows inside the imported period.
fn series_sql(resolution: Resolution, series: Series, ids: &str) -> String {
    let table = resolution.table();
    match series {
        Series::Probe(probe) => format!(
            "SELECT {NATIVE_COLUMNS} FROM {table} AS n
             WHERE n.monitor_id IN ({ids}) AND n.probe_id = {probe} AND {RANGE}",
            probe = probe.0
        ),
        Series::Monitor => format!(
            "{native}
             UNION ALL
             SELECT {KUMA_ROW} FROM kuma_stats AS n
             WHERE n.resolution = {kuma} AND n.monitor_id IN ({ids}) AND {RANGE}",
            native = native_monitor_sql(resolution, ids),
            kuma = resolution.kuma_code(),
        ),
    }
}

/// The native rows of the monitor series, after the end of each monitor's imported Kuma rows
/// of the same resolution.
fn native_monitor_sql(resolution: Resolution, ids: &str) -> String {
    let table = resolution.table();
    let kuma = resolution.kuma_code();
    let step = resolution.step_ms();
    format!(
        "WITH imported AS (
             SELECT monitor_id, max(bucket) + to_milliseconds({step}) AS until
             FROM kuma_stats WHERE resolution = {kuma} AND monitor_id IN ({ids})
             GROUP BY monitor_id
         )
         SELECT {NATIVE_COLUMNS} FROM {table} AS n
         LEFT JOIN imported AS i ON i.monitor_id = n.monitor_id
         WHERE n.monitor_id IN ({ids}) AND n.probe_id IS NULL AND {RANGE}
           AND (i.until IS NULL OR n.bucket >= i.until)"
    )
}

/// Kuma `stat_*` columns as a rollup row (decision 0008): Kuma's `ping` averages up beats,
/// so `latency_sum = ping * up` and `latency_count = up`; the maintenance count comes from
/// `extras`, Kuma's `JSON.stringify` of its extra counters.
const KUMA_ROW: &str = "n.monitor_id, n.bucket, n.up, n.down, 0,
                        coalesce(TRY_CAST(regexp_extract(n.extras, '\"maintenance\"\\s*:\\s*(\\d+)', 1) AS INTEGER), 0),
                        n.ping_min, n.ping_max,
                        coalesce(n.ping * n.up, 0), CASE WHEN n.ping IS NULL THEN 0 ELSE n.up END";

/// Runs `rows` (bound to `[from, to)`) and sums it per monitor and, when `by_bucket`, per
/// bucket (else bucket 0).
fn summed(
    conn: &Connection,
    rows: &str,
    from_ms: i64,
    to_ms: i64,
    by_bucket: bool,
) -> duckdb::Result<Vec<(MonitorId, i64, Sum)>> {
    let bucket = if by_bucket { "epoch_ms(bucket)" } else { "0" };
    let sql = format!(
        "SELECT monitor_id, {bucket} AS b,
                sum(up)::BIGINT, sum(down)::BIGINT, sum(pending)::BIGINT, sum(maintenance)::BIGINT,
                min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count)::BIGINT
         FROM ({rows}) AS rows (monitor_id, bucket, up, down, pending, maintenance,
                                latency_min, latency_max, latency_sum, latency_count)
         GROUP BY monitor_id, b ORDER BY monitor_id, b"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([ts(from_ms), ts(to_ms)], |r| {
        Ok((
            MonitorId(r.get(0)?),
            r.get(1)?,
            Sum {
                up: r.get::<_, i64>(2)? as u64,
                down: r.get::<_, i64>(3)? as u64,
                pending: r.get::<_, i64>(4)? as u64,
                maintenance: r.get::<_, i64>(5)? as u64,
                latency_min: r.get(6)?,
                latency_max: r.get(7)?,
                latency_sum: r.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
                latency_count: r.get::<_, i64>(9)? as u64,
            },
        ))
    })?;
    rows.collect()
}

/// Rollup rows of `series` and `resolution` with bucket in `[from, to)`, summed.
/// Monitor IDs are integers formatted into the SQL; time bounds are bound parameters.
fn rollups(
    conn: &Connection,
    resolution: Resolution,
    series: Series,
    monitors: &[MonitorId],
    from_ms: i64,
    to_ms: i64,
    by_bucket: bool,
) -> duckdb::Result<Vec<(MonitorId, i64, Sum)>> {
    let rows = series_sql(resolution, series, &id_list(monitors));
    summed(conn, &rows, from_ms, to_ms, by_bucket)
}

/// Raw heartbeat columns in the order [`heartbeat_row`] reads them.
const HEARTBEAT_COLUMNS: &str = "monitor_id, probe_id, epoch_ms(time), status, latency_ms,
                                 message, retries, http_status, epoch_ms(cert_expiry)";

fn heartbeat_row(r: &duckdb::Row) -> duckdb::Result<Heartbeat> {
    Ok(Heartbeat {
        monitor: MonitorId(r.get(0)?),
        probe: r.get::<_, Option<i64>>(1)?.map(ProbeId),
        time_ms: r.get(2)?,
        status: Status::from_code(r.get(3)?).ok_or_else(|| {
            duckdb::Error::FromSqlConversionFailure(
                3,
                duckdb::types::Type::UTinyInt,
                "heartbeat status outside 0..3".into(),
            )
        })?,
        latency_ms: r.get(4)?,
        message: r.get(5)?,
        retries: r.get(6)?,
        http_status: r.get(7)?,
        cert_expiry_ms: r.get(8)?,
    })
}

/// The newest heartbeat of each monitor at or after `since_ms`. Monitors without one are
/// left out. The bound keeps the scan on recent row groups.
pub(crate) fn latest(
    conn: &Connection,
    monitors: &[MonitorId],
    since_ms: i64,
) -> duckdb::Result<Vec<Heartbeat>> {
    if monitors.is_empty() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {HEARTBEAT_COLUMNS}
         FROM heartbeat WHERE monitor_id IN ({}) AND time >= ?
         QUALIFY row_number() OVER (PARTITION BY monitor_id ORDER BY time DESC, probe_id) = 1
         ORDER BY monitor_id",
        id_list(monitors)
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([ts(since_ms)], heartbeat_row)?;
    rows.collect()
}

/// Where a heartbeat page ends. Pages run newest first, by time and then by probe, with a
/// heartbeat without probe after every probe's at the same time. A probe records at most one
/// heartbeat of a monitor per millisecond, so the position is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeartbeatCursor {
    pub time_ms: i64,
    pub probe: Option<ProbeId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeartbeatPage {
    pub heartbeats: Vec<Heartbeat>,
    /// Where the next page starts; `None` on the last page.
    pub next: Option<HeartbeatCursor>,
}

/// Up to `limit` raw heartbeats of `monitor`, from `probe` only when given, after `after`.
pub(crate) fn heartbeats(
    conn: &Connection,
    monitor: MonitorId,
    probe: Option<ProbeId>,
    limit: u32,
    after: Option<HeartbeatCursor>,
) -> duckdb::Result<HeartbeatPage> {
    let mut sql = format!("SELECT {HEARTBEAT_COLUMNS} FROM heartbeat WHERE monitor_id = ?");
    let mut params = vec![Value::BigInt(monitor.0)];
    if let Some(probe) = probe {
        sql.push_str(" AND probe_id = ?");
        params.push(Value::BigInt(probe.0));
    }
    match after {
        Some(HeartbeatCursor {
            time_ms,
            probe: Some(probe),
        }) => {
            sql.push_str(" AND (time < ? OR (time = ? AND (probe_id < ? OR probe_id IS NULL)))");
            params.extend([ts(time_ms), ts(time_ms), Value::BigInt(probe.0)]);
        }
        Some(HeartbeatCursor {
            time_ms,
            probe: None,
        }) => {
            sql.push_str(" AND time < ?");
            params.push(ts(time_ms));
        }
        None => {}
    }
    sql.push_str(" ORDER BY time DESC, probe_id DESC NULLS LAST LIMIT ?");
    params.push(Value::BigInt(i64::from(limit)));
    let mut statement = conn.prepare(&sql)?;
    let heartbeats: Vec<Heartbeat> = statement
        .query_map(duckdb::params_from_iter(params), heartbeat_row)?
        .collect::<duckdb::Result<_>>()?;
    let next = (heartbeats.len() == limit as usize)
        .then(|| heartbeats.last())
        .flatten()
        .map(|last| HeartbeatCursor {
            time_ms: last.time_ms,
            probe: last.probe,
        });
    Ok(HeartbeatPage { heartbeats, next })
}

/// Uptime counts per monitor for the window ending in the bucket that holds `now_ms`.
pub(crate) fn uptime(
    conn: &Connection,
    monitors: &[MonitorId],
    series: Series,
    window: UptimeWindow,
    now_ms: i64,
) -> duckdb::Result<BTreeMap<MonitorId, Counts>> {
    if monitors.is_empty() {
        return Ok(BTreeMap::new());
    }
    let (resolution, span) = window.resolution_and_span();
    let step = resolution.step_ms();
    let to = now_ms.div_euclid(step) * step + step;
    Ok(
        rollups(conn, resolution, series, monitors, to - span, to, false)?
            .into_iter()
            .map(|(monitor, _, sum)| (monitor, sum.counts()))
            .collect(),
    )
}

/// Daily bars for the `days` local days ending with `through` in `timezone`, built from hour
/// rollups. Each UTC hour counts for the local day its start falls on, with the zone's offset
/// minutes dropped (decision 0008): India +05:30 and Nepal +05:45 use +05:00, Newfoundland
/// -03:30 uses -03:00.
///
/// For the monitor series, a day that starts before the monitor's first hourly row (Kuma
/// keeps hourly rows for a limited time) takes Kuma's `stat_daily` row for the same date, a
/// UTC day, and is marked `utc_approximate`. Native hours of that day are added: Kuma's row
/// ends at the import, so the day of the cutover, often today, goes on counting. Days
/// without data are left out.
pub(crate) fn day_bars(
    conn: &Connection,
    monitors: &[MonitorId],
    series: Series,
    timezone: Tz,
    through: NaiveDate,
    days: u32,
) -> duckdb::Result<BTreeMap<MonitorId, Vec<DayBar>>> {
    let first = through
        .checked_sub_days(Days::new(u64::from(days.saturating_sub(1))))
        .unwrap_or(NaiveDate::MIN);
    if monitors.is_empty() || days == 0 {
        return Ok(BTreeMap::new());
    }
    let local_day = |ms: i64| {
        let utc = DateTime::from_timestamp_millis(ms).expect("bucket in range");
        let offset = utc
            .with_timezone(&timezone)
            .offset()
            .fix()
            .local_minus_utc();
        // Integer division truncates toward zero, which drops the minutes on either side.
        let whole_hours = i64::from(offset / 3600 * 3600);
        (utc + chrono::Duration::seconds(whole_hours)).date_naive()
    };
    let utc_midnight = |date: NaiveDate| {
        date.and_hms_opt(0, 0, 0)
            .expect("midnight exists")
            .and_utc()
            .timestamp_millis()
    };
    // Every UTC offset lies within a day, so one day of margin covers the local range.
    let from = utc_midnight(first) - DAY_MS;
    let to = utc_midnight(through) + 2 * DAY_MS;
    let mut hourly: BTreeMap<(MonitorId, NaiveDate), Sum> = BTreeMap::new();
    // The last local day each monitor's hourly rows do not fully cover.
    let mut partial_until: BTreeMap<MonitorId, NaiveDate> = BTreeMap::new();
    for (monitor, hour, sum) in rollups(conn, Resolution::Hour, series, monitors, from, to, true)? {
        partial_until
            .entry(monitor)
            .or_insert_with(|| local_day(hour - HOUR_MS));
        let day = local_day(hour);
        if (first..=through).contains(&day) {
            hourly.entry((monitor, day)).or_default().add(&sum);
        }
    }
    let mut daily: BTreeMap<(MonitorId, NaiveDate), Sum> = BTreeMap::new();
    if series == Series::Monitor {
        let rows = format!(
            "SELECT {KUMA_ROW} FROM kuma_stats AS n
             WHERE n.resolution = {kuma} AND n.monitor_id IN ({ids})
               AND n.bucket >= $1 AND n.bucket < $2",
            kuma = Resolution::Day.kuma_code(),
            ids = id_list(monitors),
        );
        let (first_utc, after_utc) = (utc_midnight(first), utc_midnight(through) + DAY_MS);
        for (monitor, bucket, sum) in summed(conn, &rows, first_utc, after_utc, true)? {
            let utc_day = DateTime::from_timestamp_millis(bucket)
                .expect("bucket in range")
                .date_naive();
            let before_hourly = partial_until
                .get(&monitor)
                .is_none_or(|until| utc_day <= *until);
            if before_hourly {
                daily.insert((monitor, utc_day), sum);
            }
        }
        if !daily.is_empty() {
            let native = native_monitor_sql(Resolution::Hour, &id_list(monitors));
            for (monitor, hour, sum) in summed(conn, &native, from, to, true)? {
                if let Some(day) = daily.get_mut(&(monitor, local_day(hour))) {
                    day.add(&sum);
                }
            }
        }
    }
    let mut bars: BTreeMap<MonitorId, Vec<DayBar>> = BTreeMap::new();
    let days: std::collections::BTreeSet<(MonitorId, NaiveDate)> =
        hourly.keys().chain(daily.keys()).copied().collect();
    for key @ (monitor, day) in days {
        let (sum, utc_approximate) = match daily.get(&key) {
            Some(sum) => (sum, true),
            None => (&hourly[&key], false),
        };
        bars.entry(monitor).or_default().push(DayBar {
            day,
            counts: sum.counts(),
            utc_approximate,
        });
    }
    Ok(bars)
}

/// Latency of one monitor's series per bucket in `[from_ms, to_ms)`: up heartbeats, or the
/// evaluator's representative latency for the monitor series. Buckets without one are left
/// out.
pub(crate) fn latency(
    conn: &Connection,
    monitor: MonitorId,
    series: Series,
    resolution: Resolution,
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<Vec<LatencyPoint>> {
    Ok(
        rollups(conn, resolution, series, &[monitor], from_ms, to_ms, true)?
            .into_iter()
            .filter_map(|(_, bucket_ms, sum)| {
                Some(LatencyPoint {
                    bucket_ms,
                    latency: sum.latency()?,
                })
            })
            .collect(),
    )
}

/// Counts and latency of one monitor's series per bucket in `[from_ms, to_ms)`, oldest
/// first. Buckets without beats are left out.
pub(crate) fn buckets(
    conn: &Connection,
    monitor: MonitorId,
    series: Series,
    resolution: Resolution,
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<Vec<(i64, Counts)>> {
    Ok(
        rollups(conn, resolution, series, &[monitor], from_ms, to_ms, true)?
            .into_iter()
            .map(|(_, bucket_ms, sum)| (bucket_ms, sum.counts()))
            .collect(),
    )
}

/// `[from, to)` of the last `hours` UTC hours, the hour holding `now_ms` last.
fn hour_window(now_ms: i64, hours: u32) -> (i64, i64) {
    let to = now_ms.div_euclid(HOUR_MS) * HOUR_MS + HOUR_MS;
    (to - i64::from(hours) * HOUR_MS, to)
}

/// The monitor series of `monitors` summed per monitor and UTC hour in `[from, to)`, in one
/// query; hours without beats have no row.
fn hour_sums(
    conn: &Connection,
    monitors: &[MonitorId],
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<Vec<(MonitorId, i64, Sum)>> {
    if monitors.is_empty() {
        return Ok(Vec::new());
    }
    rollups(
        conn,
        Resolution::Hour,
        Series::Monitor,
        monitors,
        from_ms,
        to_ms,
        true,
    )
}

/// The last `hours` UTC hours of the monitor series of `monitors` taken together, the hour
/// holding `now_ms` last. Each monitor is classified per hour by its worst beat. Hours
/// without data are present with zero counts.
pub(crate) fn trend(
    conn: &Connection,
    monitors: &[MonitorId],
    now_ms: i64,
    hours: u32,
) -> duckdb::Result<Vec<TrendBucket>> {
    let (from, to) = hour_window(now_ms, hours);
    let rows = hour_sums(conn, monitors, from, to)?;
    let mut hours: BTreeMap<i64, (Sum, TrendBucket)> = (from..to)
        .step_by(HOUR_MS as usize)
        .map(|bucket_ms| {
            let bucket = TrendBucket {
                bucket_ms,
                ..TrendBucket::default()
            };
            (bucket_ms, (Sum::default(), bucket))
        })
        .collect();
    for (_, bucket_ms, sum) in rows {
        let Some((total, bucket)) = hours.get_mut(&bucket_ms) else {
            continue;
        };
        total.add(&sum);
        if sum.down > 0 {
            bucket.down += 1;
        } else if sum.pending > 0 {
            bucket.pending += 1;
        } else if sum.up + sum.maintenance > 0 {
            bucket.healthy += 1;
            if sum.maintenance > 0 {
                bucket.maintenance += 1;
            }
        }
    }
    Ok(hours
        .into_values()
        .map(|(total, bucket)| TrendBucket {
            counts: total.counts(),
            ..bucket
        })
        .collect())
}

/// The last `hours` UTC hours of each monitor's monitor series, the hour holding `now_ms`
/// last. Every monitor asked for is present with `hours` entries; hours without data have
/// zero counts.
pub(crate) fn hourly(
    conn: &Connection,
    monitors: &[MonitorId],
    now_ms: i64,
    hours: u32,
) -> duckdb::Result<Hourly> {
    let (from_ms, to_ms) = hour_window(now_ms, hours);
    let rows = hour_sums(conn, monitors, from_ms, to_ms)?;
    let mut counts: BTreeMap<MonitorId, Vec<Counts>> = monitors
        .iter()
        .map(|&monitor| (monitor, vec![Counts::default(); hours as usize]))
        .collect();
    for (monitor, bucket_ms, sum) in rows {
        let hour = usize::try_from((bucket_ms - from_ms) / HOUR_MS).ok();
        if let Some(slot) = counts
            .get_mut(&monitor)
            .zip(hour)
            .and_then(|(hours, hour)| hours.get_mut(hour))
        {
            *slot = sum.counts();
        }
    }
    Ok(Hourly { from_ms, counts })
}

/// Rows the Kuma import wrote for a set of monitors: raw heartbeats of the `kuma-import`
/// pseudo probe and imported `stat_*` rows per resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportedRows {
    pub heartbeats: u64,
    pub minute: u64,
    pub hour: u64,
    pub day: u64,
}

impl ImportedRows {
    pub fn stats(&self, resolution: Resolution) -> u64 {
        match resolution {
            Resolution::Minute => self.minute,
            Resolution::Hour => self.hour,
            Resolution::Day => self.day,
        }
    }
}

pub(crate) fn imported_rows(
    conn: &Connection,
    monitors: &[MonitorId],
) -> duckdb::Result<ImportedRows> {
    if monitors.is_empty() {
        return Ok(ImportedRows::default());
    }
    let ids = id_list(monitors);
    let heartbeats: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM heartbeat WHERE probe_id = {} AND monitor_id IN ({ids})",
            ProbeId::KUMA_IMPORT.0
        ),
        [],
        |row| row.get(0),
    )?;
    let mut rows = ImportedRows {
        heartbeats: heartbeats as u64,
        ..ImportedRows::default()
    };
    let mut statement = conn.prepare(&format!(
        "SELECT resolution, count(*) FROM kuma_stats WHERE monitor_id IN ({ids}) GROUP BY resolution"
    ))?;
    let counts =
        statement.query_map([], |row| Ok((row.get::<_, u8>(0)?, row.get::<_, i64>(1)?)))?;
    for count in counts {
        let (code, n) = count?;
        if let Some(resolution) = Resolution::ALL.into_iter().find(|r| r.kuma_code() == code) {
            match resolution {
                Resolution::Minute => rows.minute = n as u64,
                Resolution::Hour => rows.hour = n as u64,
                Resolution::Day => rows.day = n as u64,
            }
        }
    }
    Ok(rows)
}
