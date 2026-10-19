//! Heartbeats and `stat_*` rollups, produced the way Kuma 2.5.0 produces them.
//!
//! Beats follow the check loop in `server/model/monitor.js` (retries, upside down, important
//! flags, resend counts) and the push router in `server/routers/api-router.js`. Rollups replay
//! the beats through a copy of `server/uptime-calculator.js`, including its running average and
//! its retention: minute rows for 24 hours, hour rows for 30 days, day rows for all history.

use std::collections::BTreeMap;
use std::io::Write;

use base64::Engine;
use rusqlite::{Connection, types::Value};

use crate::catalog::{Monitor, up_message};
use crate::rng::Rng;
use crate::{Result, SEED, now};

const DOWN: i64 = 0;
const UP: i64 = 1;
const PENDING: i64 = 2;
const MAINTENANCE: i64 = 3;

const SECOND: i64 = 1000;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

/// How a monitor's raw check fails, before retries and upside down.
pub(crate) enum Outages {
    None,
    /// On average `per_day` outages a day, each 2 to 45 minutes long.
    Random {
        per_day: f64,
    },
    /// Failing since `ms_before_now` until the end of history.
    Since {
        ms_before_now: i64,
        message: &'static str,
    },
    Always {
        message: &'static str,
    },
}

/// The history a monitor gets.
pub(crate) enum Plan {
    Nothing,
    /// A checked monitor over the last `hours`, run through Kuma's retry rules.
    Checked {
        hours: i64,
        outages: Outages,
        latency_ms: i64,
        maintenance: fn(i64) -> bool,
    },
    /// A status the monitor type sets itself (empty group, manual without status).
    Pending {
        hours: i64,
        message: &'static str,
    },
    /// A push monitor fed by an hourly job, with missed and failed runs.
    Push {
        hours: i64,
    },
}

/// Recent history length of monitors without a long history.
const RECENT_HOURS: i64 = 6;

impl Plan {
    pub(crate) fn recent() -> Self {
        Self::checked(RECENT_HOURS, Outages::None)
    }

    pub(crate) fn checked(hours: i64, outages: Outages) -> Self {
        Self::Checked {
            hours,
            outages,
            latency_ms: 0,
            maintenance: |_| false,
        }
    }

    pub(crate) fn history(
        hours: i64,
        outages_per_day: f64,
        latency_ms: i64,
        maintenance: fn(i64) -> bool,
    ) -> Self {
        Self::Checked {
            hours,
            outages: Outages::Random {
                per_day: outages_per_day,
            },
            latency_ms,
            maintenance,
        }
    }

    pub(crate) fn constant(hours: i64, message: &'static str) -> Self {
        Self::Pending { hours, message }
    }
}

struct Beat {
    monitor_id: i64,
    time: i64,
    status: i64,
    message: String,
    ping: Option<f64>,
    important: bool,
    duration: i64,
    down_count: i64,
    retries: i64,
    end_time: i64,
    response: Option<String>,
}

/// `YYYY-MM-DD HH:MM:SS.mmm` in UTC, Kuma's heartbeat time format.
pub(crate) fn sql_time_ms(time_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(time_ms)
        .expect("fixture times are in range")
        .format("%Y-%m-%d %H:%M:%S%.3f")
        .to_string()
}

/// `Monitor.isImportantBeat` (`server/model/monitor.js:1385-1411`).
fn is_important(previous: Option<i64>, current: i64) -> bool {
    match previous {
        None => true,
        Some(previous) => matches!(
            (previous, current),
            (DOWN, MAINTENANCE)
                | (UP, MAINTENANCE)
                | (MAINTENANCE, DOWN)
                | (MAINTENANCE, UP)
                | (UP, DOWN)
                | (DOWN, UP)
                | (PENDING, DOWN)
        ),
    }
}

/// Carries `important` and `downCount` from the previous beat, as both code paths do.
struct Tracker {
    previous: Option<(i64, i64)>,
    resend_interval: i64,
}

impl Tracker {
    fn mark(&mut self, beat: &mut Beat) {
        beat.important = is_important(self.previous.map(|(status, _)| status), beat.status);
        let mut down_count = self.previous.map_or(0, |(_, count)| count);
        if beat.important {
            down_count = 0;
        } else if beat.status == DOWN && self.resend_interval > 0 {
            down_count += 1;
            if down_count >= self.resend_interval {
                down_count = 0;
            }
        }
        beat.down_count = down_count;
        self.previous = Some((beat.status, down_count));
    }
}

const FAILURES: [&str; 4] = [
    "timeout of 48000ms exceeded",
    "Request failed with status code 503",
    "connect ECONNREFUSED 203.0.113.10:443",
    "getaddrinfo EAI_AGAIN",
];

fn checked(
    monitor: &Monitor,
    hours: i64,
    outages: &Outages,
    latency_ms: i64,
    maintenance: fn(i64) -> bool,
    error_body: &str,
    rng: &mut Rng,
) -> Vec<Beat> {
    let end = now().timestamp_millis();
    let start = end - hours * HOUR;
    let windows: Vec<(i64, i64, &str)> = match outages {
        Outages::Random { per_day } => {
            let mut windows = Vec::new();
            for day in 0..(hours + 23) / 24 {
                if rng.chance(*per_day) {
                    let from = start + day * DAY + rng.below(DAY as u64) as i64;
                    let to = from + rng.range(2, 45) * MINUTE;
                    windows.push((
                        from,
                        to,
                        FAILURES[rng.below(FAILURES.len() as u64) as usize],
                    ));
                }
            }
            windows
        }
        Outages::Since {
            ms_before_now,
            message,
        } => vec![(end - ms_before_now, end, *message)],
        Outages::Always { message } => vec![(start, end, *message)],
        Outages::None => Vec::new(),
    };
    let base_latency = match (latency_ms, monitor.kind) {
        (0, "group" | "manual") => 2,
        (0, _) => 80,
        (ms, _) => ms,
    };
    let http_like = matches!(monitor.kind, "http" | "keyword" | "json-query");
    let real_ping = monitor.kind == "ping";

    let mut tracker = Tracker {
        previous: None,
        resend_interval: monitor.resend_interval,
    };
    let mut retries = 0;
    let mut beats = Vec::new();
    let mut time = start + rng.below((monitor.interval * SECOND) as u64) as i64;
    while time < end {
        let failure = windows
            .iter()
            .find(|(from, to, _)| (*from..*to).contains(&time))
            .map(|(_, _, m)| *m);
        let latency = {
            let spike = if rng.chance(0.01) { 5.0 } else { 1.0 };
            let value = base_latency as f64 * (0.7 + 0.6 * rng.below(1000) as f64 / 1000.0) * spike;
            if real_ping {
                (value * 100.0).round() / 100.0
            } else {
                value.round()
            }
        };

        let mut beat = Beat {
            monitor_id: monitor.id,
            time,
            status: DOWN,
            message: String::new(),
            ping: None,
            important: false,
            duration: 0,
            down_count: 0,
            retries: 0,
            end_time: time,
            response: None,
        };

        if maintenance(time) {
            beat.status = MAINTENANCE;
            beat.message = "Monitor under maintenance".into();
        } else {
            // monitor.js:456-461 and :905-911: the default status and the check result are
            // both flipped for upside-down monitors; a flipped success fails.
            let raw_up = failure.is_none();
            let passes = raw_up != monitor.upside_down;
            if passes {
                beat.status = UP;
                beat.message = match failure {
                    Some(message) => message.to_owned(),
                    None => up_message(monitor.kind).to_owned(),
                };
                beat.ping = raw_up.then_some(latency);
                retries = 0;
            } else {
                beat.message = failure.unwrap_or("Flip UP to DOWN").to_owned();
                if http_like && beat.message.contains("status code 503") {
                    beat.response = Some(error_body.to_owned());
                }
                // monitor.js:946-955
                if monitor.maxretries > 0 && retries < monitor.maxretries {
                    beat.status = PENDING;
                }
                retries += 1;
            }
        }
        beat.retries = retries;
        beat.end_time = time
            + match beat.ping {
                Some(ping) => ping.ceil() as i64 + 3,
                None if beat.message.starts_with("timeout") => 48 * SECOND,
                None => 5,
            };
        tracker.mark(&mut beat);

        let wait = if beat.status == PENDING && monitor.retry_interval > 0 {
            monitor.retry_interval
        } else {
            monitor.interval
        };
        // monitor.js:1077: the next check is `interval` after the start of this one.
        time += wait * SECOND + rng.below(40) as i64;
        beats.push(beat);
    }
    beats
}

fn pending(monitor: &Monitor, hours: i64, message: &str, rng: &mut Rng) -> Vec<Beat> {
    let end = now().timestamp_millis();
    let mut tracker = Tracker {
        previous: None,
        resend_interval: monitor.resend_interval,
    };
    let mut time = end - hours * HOUR + rng.below((monitor.interval * SECOND) as u64) as i64;
    let mut beats = Vec::new();
    while time < end {
        let mut beat = Beat {
            monitor_id: monitor.id,
            time,
            status: PENDING,
            message: message.to_owned(),
            ping: Some(rng.range(0, 3) as f64),
            important: false,
            duration: 0,
            down_count: 0,
            retries: 0,
            end_time: time + 3,
            response: None,
        };
        tracker.mark(&mut beat);
        beats.push(beat);
        time += monitor.interval * SECOND + rng.below(40) as i64;
    }
    beats
}

/// An hourly job pushing to `/api/push/:token`, with the server's deadline loop filling gaps.
fn push(monitor: &Monitor, hours: i64, rng: &mut Rng) -> Vec<Beat> {
    let end = now().timestamp_millis();
    let interval = monitor.interval * SECOND;

    // The job runs at five seconds past each interval; now and then it misses a few runs or
    // reports a failure with `status=down`.
    let mut pushes = Vec::new();
    let mut time = end - hours * HOUR + 5 * SECOND;
    while time < end {
        if rng.chance(0.004) {
            time += rng.range(1, 4) * interval;
            continue;
        }
        pushes.push((time + rng.below(800) as i64, rng.chance(0.003)));
        time += interval;
    }

    let mut tracker = Tracker {
        previous: None,
        resend_interval: monitor.resend_interval,
    };
    let mut beats: Vec<Beat> = Vec::new();
    let mut pushes = pushes.into_iter().peekable();
    loop {
        // monitor.js:746-777: the loop records a failure once the latest beat is not up or is
        // older than `interval` plus the 1 s buffer, which it sees on the tick after the deadline.
        let deadline = beats.last().map(|last| match last.status {
            UP => last.time + interval + 2 * SECOND,
            PENDING => last.time + monitor.retry_interval * SECOND,
            _ => last.time + interval,
        });
        let next_push = pushes.peek().copied();
        let (time, failed_push) = match (deadline, next_push) {
            (Some(deadline), Some((push, _))) if deadline < push => (deadline, None),
            (Some(deadline), None) => (deadline, None),
            (_, Some((push, failed))) => (push, Some(failed)),
            (None, None) => break,
        };
        if time >= end {
            break;
        }

        let previous = beats.last();
        let mut beat = Beat {
            monitor_id: monitor.id,
            time,
            status: UP,
            message: String::new(),
            ping: None,
            important: false,
            duration: 0,
            down_count: 0,
            retries: 0,
            end_time: time + 2,
            response: None,
        };

        if let Some(failed) = failed_push {
            pushes.next();
            beat.message = if failed {
                "backup failed: no space left on device"
            } else {
                "OK"
            }
            .into();
            beat.ping = Some(rng.range(400, 2600) as f64);
            beat.duration = previous.map_or(0, |p| (time - p.time) / SECOND);
            // api-router.js:576-619 (determineStatus)
            let status = if failed { DOWN } else { UP };
            (beat.status, beat.retries) = match (previous.map(|p| (p.status, p.retries)), status) {
                (Some((UP, retries)), DOWN)
                    if monitor.maxretries > 0 && retries < monitor.maxretries =>
                {
                    (PENDING, retries + 1)
                }
                (Some((UP, _)), DOWN) => (DOWN, 0),
                (Some((PENDING, retries)), DOWN) if retries < monitor.maxretries => {
                    (PENDING, retries + 1)
                }
                (Some((_, retries)), DOWN) => (DOWN, retries + 1),
                (Some(_), _) => (UP, 0),
                (None, DOWN) if monitor.maxretries > 0 => (PENDING, 1),
                (None, status) => (status, 0),
            };
        } else {
            let previous = previous.expect("the loop only fires after a beat");
            beat.message = "No heartbeat in the time window".into();
            beat.duration = ((time - previous.time) as f64 / SECOND as f64).round() as i64;
            beat.status = if monitor.maxretries > 0 && previous.retries < monitor.maxretries {
                PENDING
            } else {
                DOWN
            };
            beat.retries = previous.retries + 1;
        }
        tracker.mark(&mut beat);
        beats.push(beat);
    }
    beats
}

/// One bucket of `server/uptime-calculator.js`.
#[derive(Default)]
struct Bucket {
    up: i64,
    down: i64,
    maintenance: i64,
    avg_ping: f64,
    min_ping: f64,
    max_ping: f64,
}

impl Bucket {
    /// `UptimeCalculator.update` (`server/uptime-calculator.js:231-282`).
    fn add(&mut self, status: i64, ping: Option<f64>) {
        match status {
            MAINTENANCE => self.maintenance += 1,
            UP => {
                self.up += 1;
                if let Some(ping) = ping {
                    if self.up == 1 {
                        (self.avg_ping, self.min_ping, self.max_ping) = (ping, ping, ping);
                    } else {
                        self.avg_ping =
                            (self.avg_ping * (self.up - 1) as f64 + ping) / self.up as f64;
                        self.min_ping = self.min_ping.min(ping);
                        self.max_ping = self.max_ping.max(ping);
                    }
                }
            }
            _ => self.down += 1,
        }
    }
}

type Buckets = BTreeMap<i64, Bucket>;

fn insert_stats(
    conn: &Connection,
    table: &str,
    monitor_id: i64,
    buckets: &Buckets,
    keep_from: i64,
) -> Result<()> {
    let mut stmt = conn.prepare_cached(&format!(
        "INSERT INTO {table} (monitor_id, timestamp, ping, ping_min, ping_max, up, down, extras)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
    ))?;
    for (key, bucket) in buckets.range(keep_from..) {
        let extras =
            (bucket.maintenance > 0).then(|| format!("{{\"maintenance\":{}}}", bucket.maintenance));
        stmt.execute(rusqlite::params![
            monitor_id,
            key,
            bucket.avg_ping,
            bucket.min_ping,
            bucket.max_ping,
            bucket.up,
            bucket.down,
            extras
        ])?;
    }
    Ok(())
}

/// Brotli and base64, as `Monitor.saveResponseData` stores a response body.
fn encode_response(body: &str) -> Result<String> {
    let mut compressed = Vec::new();
    {
        let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 11, 22);
        writer.write_all(body.as_bytes())?;
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(compressed))
}

pub(crate) fn insert(conn: &Connection, monitors: &[Monitor]) -> Result<()> {
    let mut rng = Rng::new(SEED);
    let error_body = encode_response("<html><body><h1>503 Service Unavailable</h1></body></html>")?;
    let mut all = Vec::new();

    for monitor in monitors {
        let beats = match &monitor.plan {
            Plan::Nothing => continue,
            Plan::Checked {
                hours,
                outages,
                latency_ms,
                maintenance,
            } => checked(
                monitor,
                *hours,
                outages,
                *latency_ms,
                *maintenance,
                &error_body,
                &mut rng,
            ),
            Plan::Pending { hours, message } => pending(monitor, *hours, message, &mut rng),
            Plan::Push { hours } => push(monitor, *hours, &mut rng),
        };
        let Some(last) = beats.last() else { continue };

        let (mut minutes, mut hours, mut days) = (Buckets::new(), Buckets::new(), Buckets::new());
        for beat in &beats {
            let seconds = beat.time.div_euclid(SECOND);
            minutes
                .entry(seconds - seconds % 60)
                .or_default()
                .add(beat.status, beat.ping);
            hours
                .entry(seconds - seconds % 3600)
                .or_default()
                .add(beat.status, beat.ping);
            days.entry(seconds - seconds % 86_400)
                .or_default()
                .add(beat.status, beat.ping);
        }
        // Retention applied on every beat (uptime-calculator.js:357-371), seen from the last one.
        let last_seconds = last.time / SECOND;
        let minute_floor = (last_seconds - 86_400) - (last_seconds - 86_400) % 60;
        let hour_floor = (last_seconds - 30 * 86_400) - (last_seconds - 30 * 86_400) % 3600;
        insert_stats(conn, "stat_minutely", monitor.id, &minutes, minute_floor)?;
        insert_stats(conn, "stat_hourly", monitor.id, &hours, hour_floor)?;
        insert_stats(conn, "stat_daily", monitor.id, &days, i64::MIN)?;
        all.extend(beats);
    }

    // Kuma assigns heartbeat IDs in time order across all monitors.
    all.sort_by_key(|beat| (beat.time, beat.monitor_id));
    let real_ping: Vec<i64> = monitors
        .iter()
        .filter(|m| m.kind == "ping")
        .map(|m| m.id)
        .collect();
    let mut stmt = conn.prepare_cached(
        "INSERT INTO heartbeat (important, monitor_id, status, msg, time, ping, duration, down_count, end_time, retries, response)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )?;
    for beat in all {
        let ping = match beat.ping {
            Some(ping) if real_ping.contains(&beat.monitor_id) => Value::Real(ping),
            Some(ping) => Value::Integer(ping as i64),
            None => Value::Null,
        };
        stmt.execute(rusqlite::params![
            beat.important,
            beat.monitor_id,
            beat.status,
            beat.message,
            sql_time_ms(beat.time),
            ping,
            beat.duration,
            beat.down_count,
            sql_time_ms(beat.end_time),
            beat.retries,
            beat.response
        ])?;
    }
    Ok(())
}
