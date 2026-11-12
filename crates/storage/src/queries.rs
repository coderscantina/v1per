//! Bounded typed reads over rollups (monitoring contract, Uptime), and the latest raw row.
//!
//! Rollups are kept per (monitor, probe); probe NULL is the monitor-level series. Reads of
//! that series combine native rollups with imported Kuma `stat_*` rows of the same
//! resolution. Per monitor, Kuma rows cover everything up to the end of its last imported
//! bucket and native rows count only from there on, so uptime for imported periods matches
//! Kuma.

use std::collections::{BTreeMap, BTreeSet};

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

/// One monitor's monitor-level beats over a period, for SLA availability and latency.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PeriodTotals {
    pub up: u64,
    /// Down beats with pending folded in, as page uptime counts them.
    pub down: u64,
    /// The pending part of `down`, for display. Imported Kuma rows carry none.
    pub pending: u64,
    pub maintenance: u64,
    /// Latency of up beats; the average is `latency_sum / latency_count`.
    pub latency_sum: f64,
    pub latency_count: u64,
    /// Any rollup row fell in the period.
    pub has_data: bool,
    /// A Kuma day row counted whole although the period ends inside its UTC day.
    pub utc_approximate: bool,
}

impl PeriodTotals {
    /// `up / (up + down)`; `None` without an up or down beat. Equal to [`Counts::uptime`].
    pub fn uptime(&self) -> Option<f64> {
        let total = self.up + self.down;
        (total > 0).then(|| self.up as f64 / total as f64)
    }
}

/// Totals per requested monitor over `[from_ms, to_ms)`, whose edges are whole UTC hours.
///
/// Reads the monitor series from hour rollups, Kuma's hourly rows included. Like
/// [`day_bars`], a UTC day that starts before the monitor's first hourly row in the range
/// takes Kuma's `stat_daily` row plus the native hours of that day. A Kuma day row counts
/// whole when its UTC day starts inside the range, so consecutive periods never count one
/// twice. An edge inside such a day cannot be split: the row is used whole and the totals
/// are marked `utc_approximate` when the day reaches past `to_ms`.
pub(crate) fn period_totals(
    conn: &Connection,
    monitors: &[MonitorId],
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<BTreeMap<MonitorId, PeriodTotals>> {
    if monitors.is_empty() || from_ms >= to_ms {
        return Ok(BTreeMap::new());
    }
    let mut totals: BTreeMap<MonitorId, (Sum, bool)> = monitors
        .iter()
        .map(|&monitor| (monitor, (Sum::default(), false)))
        .collect();
    let ids = id_list(monitors);
    let hours = summed(
        conn,
        &series_sql(Resolution::Hour, Series::Monitor, &ids),
        from_ms,
        to_ms,
        true,
    )?;
    let mut first_hour: BTreeMap<MonitorId, i64> = BTreeMap::new();
    for (monitor, hour, _) in &hours {
        first_hour.entry(*monitor).or_insert(*hour);
    }
    let day_start = |ms: i64| ms.div_euclid(DAY_MS) * DAY_MS;
    let kuma_days = format!(
        "SELECT {KUMA_ROW} FROM kuma_stats AS n
         WHERE n.resolution = {kuma} AND n.monitor_id IN ({ids}) AND {RANGE}",
        kuma = Resolution::Day.kuma_code(),
    );
    let mut daily: BTreeSet<(MonitorId, i64)> = BTreeSet::new();
    for (monitor, day, sum) in summed(conn, &kuma_days, from_ms, to_ms, true)? {
        let before_hourly = first_hour
            .get(&monitor)
            .is_none_or(|first| day <= day_start(first - HOUR_MS));
        if before_hourly {
            let (total, approximate) = totals.get_mut(&monitor).expect("asked for");
            total.add(&sum);
            *approximate |= day + DAY_MS > to_ms;
            daily.insert((monitor, day));
        }
    }
    for (monitor, hour, sum) in hours {
        if !daily.contains(&(monitor, day_start(hour))) {
            totals.get_mut(&monitor).expect("asked for").0.add(&sum);
        }
    }
    if !daily.is_empty() {
        // Kuma's day ends at the import: native hours after it still count in that day.
        let native = native_monitor_sql(Resolution::Hour, &ids);
        for (monitor, hour, sum) in summed(conn, &native, from_ms, to_ms, true)? {
            if daily.contains(&(monitor, day_start(hour))) {
                totals.get_mut(&monitor).expect("asked for").0.add(&sum);
            }
        }
    }
    Ok(totals
        .into_iter()
        .map(|(monitor, (sum, utc_approximate))| {
            let totals = PeriodTotals {
                up: sum.up,
                down: sum.down + sum.pending,
                pending: sum.pending,
                maintenance: sum.maintenance,
                latency_sum: sum.latency_sum,
                latency_count: sum.latency_count,
                has_data: sum.up + sum.down + sum.pending + sum.maintenance > 0,
                utc_approximate,
            };
            (monitor, totals)
        })
        .collect())
}

/// Minutes of a service made of several monitors, from the monitor series of the minute
/// rollups.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DownMinutes {
    /// Minutes where at least one monitor has an up or down beat (pending counts as down).
    pub counted: u64,
    /// Minutes where at least one monitor has a down or pending beat.
    pub down: u64,
    /// Minute rollups of the monitors reach back to the start of the range, so it is not
    /// missing minutes to retention or a late import. Without it the caller treats the range
    /// as no data.
    pub covered: bool,
}

/// Counted and down minutes of `monitors` taken together over `[from_ms, to_ms)`, each minute
/// once however many monitors beat in it. Maintenance-only minutes are not counted.
pub(crate) fn down_minutes(
    conn: &Connection,
    monitors: &[MonitorId],
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<DownMinutes> {
    if monitors.is_empty() || from_ms >= to_ms {
        return Ok(DownMinutes::default());
    }
    let ids = id_list(monitors);
    let rows = series_sql(Resolution::Minute, Series::Monitor, &ids);
    let (counted, down): (i64, i64) = conn.query_row(
        &format!(
            "SELECT count(DISTINCT bucket) FILTER (WHERE up + down + pending > 0),
                    count(DISTINCT bucket) FILTER (WHERE down + pending > 0)
             FROM ({rows}) AS rows (monitor_id, bucket, up, down, pending, maintenance,
                                    latency_min, latency_max, latency_sum, latency_count)"
        ),
        [ts(from_ms), ts(to_ms)],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let covered: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (
                 SELECT 1 FROM rollup_minute
                 WHERE monitor_id IN ({ids}) AND probe_id IS NULL AND bucket <= $1
                 UNION ALL
                 SELECT 1 FROM kuma_stats
                 WHERE resolution = {kuma} AND monitor_id IN ({ids}) AND bucket <= $1)",
            kuma = Resolution::Minute.kuma_code()
        ),
        [ts(from_ms)],
        |r| r.get(0),
    )?;
    Ok(DownMinutes {
        counted: counted as u64,
        down: down as u64,
        covered,
    })
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

/// The latency of up checks over a span, from raw heartbeats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencySummary {
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// Up checks with a latency.
    pub checks: u64,
}

/// One series of [`ResponseTimes`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeriesLatency {
    /// Steps with up beats, oldest first.
    pub points: Vec<LatencyPoint>,
    /// `None` without an up check in the span.
    pub summary: Option<LatencySummary>,
}

/// One monitor's response times over a span: the monitor series and probe groups.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseTimes {
    /// Points from the monitor series; the summary counts every probe's raw checks.
    pub overall: SeriesLatency,
    /// One per group asked for, in order: the group's probe series merged.
    pub groups: Vec<SeriesLatency>,
}

/// `CASE probe_id ... END`: the index of the group holding a probe, NULL outside every group.
fn group_index(groups: &[Vec<ProbeId>]) -> String {
    let arms: String = groups
        .iter()
        .enumerate()
        .flat_map(|(index, group)| group.iter().map(move |probe| (index, probe.0)))
        .map(|(index, probe)| format!(" WHEN {probe} THEN {index}"))
        .collect();
    if arms.is_empty() {
        "NULL::INTEGER".to_owned()
    } else {
        format!("CASE probe_id{arms} END")
    }
}

/// Latency points of rollup `rows` (bound to `[$1, $2)`) per `step_ms` from the bucket's UTC
/// epoch and per `group` expression, oldest first. Steps without up beats are left out.
fn stepped_latency(
    conn: &Connection,
    rows: &str,
    group: &str,
    step_ms: i64,
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<Vec<(i64, LatencyPoint)>> {
    let sql = format!(
        "SELECT {group} AS g, epoch_ms(bucket) // {step_ms} * {step_ms} AS b,
                min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count)::BIGINT
         FROM ({rows}) AS rows (probe_id, monitor_id, bucket, up, down, pending, maintenance,
                                latency_min, latency_max, latency_sum, latency_count)
         GROUP BY g, b HAVING sum(latency_count) > 0 ORDER BY g, b"
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([ts(from_ms), ts(to_ms)], |r| {
        let sum = Sum {
            latency_min: r.get(2)?,
            latency_max: r.get(3)?,
            latency_sum: r.get::<_, Option<f64>>(4)?.unwrap_or(0.0),
            latency_count: r.get::<_, i64>(5)? as u64,
            ..Sum::default()
        };
        Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, i64>(1)?, sum))
    })?;
    let mut points = Vec::new();
    for row in rows {
        let (group, bucket_ms, sum) = row?;
        if let (Some(group), Some(latency)) = (group, sum.latency()) {
            points.push((group, LatencyPoint { bucket_ms, latency }));
        }
    }
    Ok(points)
}

/// Response times of `monitor` in `[from_ms, to_ms)`: points of `resolution` rollups summed
/// per `step_ms` (a multiple of the resolution's step), for the monitor series and for each
/// group of probes merged, and the average and percentiles of the up checks in the span.
///
/// Percentiles need single checks, so they come from raw heartbeats: the overall one from
/// every probe's checks (push monitors and imported Kuma checks included), a group's from its
/// probes'. The scan is bounded by the time range and reads three columns.
pub(crate) fn response_times(
    conn: &Connection,
    monitor: MonitorId,
    groups: &[Vec<ProbeId>],
    resolution: Resolution,
    step_ms: i64,
    from_ms: i64,
    to_ms: i64,
) -> duckdb::Result<ResponseTimes> {
    let ids = monitor.0.to_string();
    let overall_rows = format!(
        "SELECT NULL, * FROM ({})",
        series_sql(resolution, Series::Monitor, &ids)
    );
    let mut answer = ResponseTimes {
        overall: SeriesLatency::default(),
        groups: vec![SeriesLatency::default(); groups.len()],
    };
    answer.overall.points = stepped_latency(conn, &overall_rows, "0", step_ms, from_ms, to_ms)?
        .into_iter()
        .map(|(_, point)| point)
        .collect();
    let probes: Vec<String> = groups.iter().flatten().map(|p| p.0.to_string()).collect();
    if !probes.is_empty() {
        let rows = format!(
            "SELECT n.probe_id, {NATIVE_COLUMNS} FROM {table} AS n
             WHERE n.monitor_id = {ids} AND n.probe_id IN ({probes}) AND {RANGE}",
            table = resolution.table(),
            probes = probes.join(","),
        );
        let index = group_index(groups);
        for (group, point) in stepped_latency(conn, &rows, &index, step_ms, from_ms, to_ms)? {
            if let Some(series) = answer.groups.get_mut(group as usize) {
                series.points.push(point);
            }
        }
    }
    let sql = format!(
        "SELECT total, g, q[1], q[2], q[3], average, checks
         FROM (SELECT GROUPING(g) AS total, g, quantile_cont(latency_ms, [0.5, 0.95, 0.99]) AS q,
                      avg(latency_ms) AS average, count(*) AS checks
               FROM (SELECT {index} AS g, latency_ms FROM heartbeat
                     WHERE monitor_id = {ids} AND time >= $1 AND time < $2
                       AND status = 1 AND latency_ms IS NOT NULL)
               GROUP BY GROUPING SETS ((g), ()) HAVING count(*) > 0)",
        index = group_index(groups),
    );
    let mut statement = conn.prepare(&sql)?;
    let rows = statement.query_map([ts(from_ms), ts(to_ms)], |r| {
        Ok((
            r.get::<_, i64>(0)? == 1,
            r.get::<_, Option<i64>>(1)?,
            LatencySummary {
                p50_ms: r.get(2)?,
                p95_ms: r.get(3)?,
                p99_ms: r.get(4)?,
                avg_ms: r.get(5)?,
                checks: r.get::<_, i64>(6)? as u64,
            },
        ))
    })?;
    for row in rows {
        match row? {
            (true, _, summary) => answer.overall.summary = Some(summary),
            (false, Some(group), summary) => {
                if let Some(series) = answer.groups.get_mut(group as usize) {
                    series.summary = Some(summary);
                }
            }
            (false, None, _) => {}
        }
    }
    Ok(answer)
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
