//! Deterministic release-gate workload for the storage gate (milestone 02): the monitor mix,
//! check results per probe with latency, outages, probe partitions, flapping and maintenance,
//! and the evaluator's monitor-level statuses.
//!
//! Every value is a pure function of the seed and the check, so any time window can be
//! generated on its own and in any order: the bulk prefill and the live run produce the same
//! results for the same window.

use serde::{Deserialize, Serialize};
use v1per_storage::{Heartbeat, MonitorId, MonitorStatus, ProbeId, Status};

pub const MINUTE_MS: i64 = 60_000;
pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 86_400_000;
const WEEK_MS: i64 = 7 * DAY_MS;
/// 2027-01-01T00:00:00Z, the base of generated certificate expiry dates.
const CERT_BASE_MS: i64 = 1_798_761_600_000;

/// The release-gate mix. Rates are per mille unless named otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub seed: u64,
    pub fast_monitors: u32,
    pub fast_interval_s: u32,
    pub slow_monitors: u32,
    pub slow_interval_s: u32,
    /// Probe IDs `1..=probes`; probe 1 is the built-in probe.
    pub probes: u32,
    /// Monitors checked from `multi_probe_count` probes: the built-in one plus remote ones.
    pub multi_probe_permille: u32,
    pub multi_probe_count: u32,
    /// Share of single-probe monitors on the built-in probe; the rest spread evenly.
    pub builtin_permille: u32,
    /// Agents send what they checked every this many seconds.
    pub agent_batch_s: u32,
    /// The evaluator records monitor-level statuses every this many seconds.
    pub evaluator_batch_s: u32,
    /// Chance per monitor and UTC day that an outage episode starts (1 to 180 minutes).
    pub outage_per_day_permille: u32,
    /// Chance per probe and UTC day of a network partition failing its checks (2 to 15 minutes).
    pub partition_per_day_permille: u32,
    /// Monitors that flap: in a fifth of their hours, 40 % of checks fail.
    pub flapping_permille: u32,
    /// Monitors with a weekly maintenance window, Sunday 02:00 to 03:00 UTC.
    pub maintenance_permille: u32,
    /// Isolated failed checks, per million checks.
    pub blip_per_million: u32,
}

impl Default for Manifest {
    /// The release gate in the technical briefing.
    fn default() -> Self {
        Self {
            seed: 0x7631_7065_7202_0001,
            fast_monitors: 400,
            fast_interval_s: 20,
            slow_monitors: 1_600,
            slow_interval_s: 60,
            probes: 10,
            multi_probe_permille: 100,
            multi_probe_count: 3,
            builtin_permille: 500,
            agent_batch_s: 5,
            evaluator_batch_s: 1,
            outage_per_day_permille: 150,
            partition_per_day_permille: 300,
            flapping_permille: 20,
            maintenance_permille: 10,
            blip_per_million: 500,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Http,
    Keyword,
    Port,
    Ping,
    Dns,
}

/// One generated monitor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub id: MonitorId,
    pub kind: Kind,
    pub interval_ms: i64,
    /// Offset of the first check within the interval, so checks spread over time.
    pub phase_ms: i64,
    pub probes: Vec<ProbeId>,
    pub max_retries: u32,
    /// Median latency from the built-in probe.
    pub base_latency_ms: f64,
    pub flapping: bool,
    pub maintenance: bool,
    pub cert_expiry_ms: Option<i64>,
}

/// Results of the checks scheduled in a time window, each sorted by time.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Window {
    pub heartbeats: Vec<Heartbeat>,
    pub statuses: Vec<MonitorStatus>,
}

pub struct Workload {
    manifest: Manifest,
    monitors: Vec<Monitor>,
}

// Independent draw streams.
const S_FAST: u64 = 1;
const S_MONITOR: u64 = 2;
const S_OUTAGE: u64 = 3;
const S_PARTITION: u64 = 4;
const S_FLAP_HOUR: u64 = 5;
const S_FLAP_CHECK: u64 = 6;
const S_BLIP: u64 = 7;
const S_LATENCY: u64 = 8;
const S_DISTANCE: u64 = 9;
const S_JITTER: u64 = 10;
const S_FAILURE: u64 = 11;

impl Workload {
    pub fn new(manifest: Manifest) -> Self {
        let count = (manifest.fast_monitors + manifest.slow_monitors) as u64;
        let seed = manifest.seed;
        // The fast monitors are a seeded selection, not the first IDs.
        let mut order: Vec<u64> = (0..count).collect();
        order.sort_by_key(|&i| draw(seed, [S_FAST, i]));
        let mut fast = vec![false; count as usize];
        for &i in &order[..manifest.fast_monitors as usize] {
            fast[i as usize] = true;
        }
        let monitors = (0..count)
            .map(|i| monitor(&manifest, i, fast[i as usize]))
            .collect();
        Self { manifest, monitors }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    /// Every check scheduled in `[from_ms, to_ms)`: one heartbeat per probe and one
    /// evaluator status per check.
    pub fn generate(&self, from_ms: i64, to_ms: i64) -> Window {
        let mut window = Window::default();
        for monitor in &self.monitors {
            let interval = monitor.interval_ms;
            let first = (from_ms - monitor.phase_ms).div_euclid(interval)
                + i64::from((from_ms - monitor.phase_ms).rem_euclid(interval) != 0);
            let mut tick = first;
            loop {
                let scheduled = tick * interval + monitor.phase_ms;
                if scheduled >= to_ms {
                    break;
                }
                let start = window.heartbeats.len();
                for &probe in &monitor.probes {
                    window
                        .heartbeats
                        .push(self.check(monitor, probe, tick, scheduled));
                }
                window
                    .statuses
                    .push(evaluate(monitor, scheduled, &window.heartbeats[start..]));
                tick += 1;
            }
        }
        window
            .heartbeats
            .sort_by_key(|h| (h.time_ms, h.monitor, h.probe));
        window.statuses.sort_by_key(|s| (s.time_ms, s.monitor));
        window
    }

    fn check(&self, monitor: &Monitor, probe: ProbeId, tick: i64, scheduled: i64) -> Heartbeat {
        let seed = self.manifest.seed;
        let (m, p, k) = (monitor.id.0 as u64, probe.0 as u64, tick as u64);
        let jitter = below(draw(seed, [S_JITTER, m, p, k]), 2_000) as i64;
        let in_maintenance = monitor.maintenance && {
            // Unix time 0 was a Thursday; Sunday 02:00 is 3 days and 2 hours into the week.
            let in_week = scheduled.rem_euclid(WEEK_MS);
            (3 * DAY_MS + 2 * HOUR_MS..3 * DAY_MS + 3 * HOUR_MS).contains(&in_week)
        };
        let (status, retries) = if in_maintenance {
            (Status::Maintenance, 0)
        } else if self.failing(monitor, probe, tick) {
            let earlier = (1..=i64::from(monitor.max_retries))
                .take_while(|back| self.failing(monitor, probe, tick - back))
                .count() as u32;
            if earlier < monitor.max_retries {
                (Status::Pending, earlier + 1)
            } else {
                (Status::Down, earlier)
            }
        } else {
            (Status::Up, 0)
        };

        let distance = if probe.0 == 1 {
            1.0
        } else {
            1.0 + below(draw(seed, [S_DISTANCE, m, p]), 150) as f64 / 100.0
        };
        let noise = normal(
            draw(seed, [S_LATENCY, m, p, k, 0]),
            draw(seed, [S_LATENCY, m, p, k, 1]),
        );
        let spike = if below(draw(seed, [S_LATENCY, m, p, k, 2]), 100) == 0 {
            3.0 + below(draw(seed, [S_LATENCY, m, p, k, 3]), 6) as f64
        } else {
            1.0
        };
        let measured = (monitor.base_latency_ms * distance * (0.25 * noise).exp() * spike)
            .clamp(1.0, 30_000.0) as u32;
        let http = matches!(monitor.kind, Kind::Http | Kind::Keyword);
        let failure = below(draw(seed, [S_FAILURE, m, p, k]), 3);
        let (latency_ms, message, http_status) = match status {
            Status::Up | Status::Maintenance => (
                Some(measured),
                if http { "200 - OK" } else { "" },
                http.then_some(200),
            ),
            Status::Down | Status::Pending => match (failure, http) {
                (0, _) => (None, "timeout of 48000ms exceeded", None),
                (1, true) => (
                    Some(measured),
                    "Request failed with status code 503",
                    Some(503),
                ),
                (1, false) => (Some(measured), "connect ECONNREFUSED", None),
                (_, true) if monitor.kind == Kind::Keyword => (
                    Some(measured),
                    "but keyword is not in [<!doctype html><html lang=\"en\"> ...]",
                    Some(200),
                ),
                _ => (None, "getaddrinfo ENOTFOUND", None),
            },
        };
        Heartbeat {
            monitor: monitor.id,
            probe: Some(probe),
            time_ms: scheduled + jitter,
            status,
            latency_ms,
            message: message.to_owned(),
            retries,
            http_status,
            cert_expiry_ms: monitor.cert_expiry_ms,
        }
    }

    /// Whether the check fails, before retries and maintenance.
    fn failing(&self, monitor: &Monitor, probe: ProbeId, tick: i64) -> bool {
        let manifest = &self.manifest;
        let seed = manifest.seed;
        let time = tick * monitor.interval_ms + monitor.phase_ms;
        let (m, p, k) = (monitor.id.0 as u64, probe.0 as u64, tick as u64);
        let outage = episode(
            seed,
            [S_OUTAGE, m],
            manifest.outage_per_day_permille,
            time,
            |u| match below(u, 100) {
                0..60 => MINUTE_MS + below(mix64(u), 4 * MINUTE_MS as u64) as i64,
                60..90 => 5 * MINUTE_MS + below(mix64(u), 25 * MINUTE_MS as u64) as i64,
                _ => 30 * MINUTE_MS + below(mix64(u), 150 * MINUTE_MS as u64) as i64,
            },
        );
        let partition = || {
            episode(
                seed,
                [S_PARTITION, p],
                manifest.partition_per_day_permille,
                time,
                |u| 2 * MINUTE_MS + below(u, 13 * MINUTE_MS as u64) as i64,
            )
        };
        let flapping = || {
            monitor.flapping
                && below(
                    draw(seed, [S_FLAP_HOUR, m, time.div_euclid(HOUR_MS) as u64]),
                    5,
                ) == 0
                && below(draw(seed, [S_FLAP_CHECK, m, p, k]), 10) < 4
        };
        let blip = || {
            below(draw(seed, [S_BLIP, m, p, k]), 1_000_000) < u64::from(manifest.blip_per_million)
        };
        outage || partition() || flapping() || blip()
    }
}

/// Whether `time` falls in an episode of the stream `key`. An episode starts on a UTC day
/// with `permille` chance, at a seeded time, and lasts `duration(draw)`, at most a day.
fn episode(
    seed: u64,
    key: [u64; 2],
    permille: u32,
    time: i64,
    duration: impl Fn(u64) -> i64,
) -> bool {
    let today = time.div_euclid(DAY_MS);
    (today - 1..=today).any(|day| {
        let d = day as u64;
        if below(draw(seed, [key[0], key[1], d, 0]), 1000) >= u64::from(permille) {
            return false;
        }
        let start = day * DAY_MS + below(draw(seed, [key[0], key[1], d, 1]), DAY_MS as u64) as i64;
        let end = start + duration(draw(seed, [key[0], key[1], d, 2])).min(DAY_MS);
        (start..end).contains(&time)
    })
}

fn monitor(manifest: &Manifest, index: u64, fast: bool) -> Monitor {
    let u = |sub: u64| draw(manifest.seed, [S_MONITOR, index, sub]);
    let kind = match below(u(0), 100) {
        0..70 => Kind::Http,
        70..80 => Kind::Keyword,
        80..90 => Kind::Port,
        90..95 => Kind::Ping,
        _ => Kind::Dns,
    };
    let interval_ms = 1000
        * i64::from(if fast {
            manifest.fast_interval_s
        } else {
            manifest.slow_interval_s
        });
    let remote = manifest.probes.saturating_sub(1) as u64;
    let multi = manifest.multi_probe_count.min(manifest.probes) as usize;
    let probes = if multi > 1 && below(u(2), 1000) < u64::from(manifest.multi_probe_permille) {
        // The built-in probe plus distinct remote probes from a seeded start.
        let start = below(u(3), remote);
        (0..multi as u64 - 1)
            .map(|n| ProbeId(2 + ((start + n) % remote) as i64))
            .chain([ProbeId(1)])
            .collect()
    } else if remote == 0 || below(u(4), 1000) < u64::from(manifest.builtin_permille) {
        vec![ProbeId(1)]
    } else {
        vec![ProbeId(2 + below(u(5), remote) as i64)]
    };
    let max_retries = match below(u(6), 10) {
        0..5 => 0,
        5..8 => 1,
        _ => 3,
    };
    let (median, spread) = match kind {
        Kind::Http | Kind::Keyword => (180.0, 0.7),
        Kind::Port | Kind::Ping => (25.0, 0.8),
        Kind::Dns => (20.0, 0.5),
    };
    let base_latency_ms = (median * (spread * normal(u(7), u(8))).exp()).clamp(1.0, 5_000.0);
    Monitor {
        id: MonitorId(index as i64 + 1),
        kind,
        interval_ms,
        phase_ms: below(u(1), interval_ms as u64) as i64,
        probes,
        max_retries,
        base_latency_ms,
        flapping: below(u(9), 1000) < u64::from(manifest.flapping_permille),
        maintenance: below(u(10), 1000) < u64::from(manifest.maintenance_permille),
        cert_expiry_ms: matches!(kind, Kind::Http | Kind::Keyword)
            .then(|| CERT_BASE_MS + below(u(11), 400) as i64 * DAY_MS),
    }
}

/// The evaluator's monitor-level status for one check: majority quorum over the probes
/// (down when at least half are down), pending when failing probes reach half otherwise,
/// and the median latency of up probes.
fn evaluate(monitor: &Monitor, scheduled: i64, beats: &[Heartbeat]) -> MonitorStatus {
    let count = |status| beats.iter().filter(|b| b.status == status).count();
    let (down, pending) = (count(Status::Down), count(Status::Pending));
    let status = if count(Status::Maintenance) > 0 {
        Status::Maintenance
    } else if down * 2 >= beats.len() {
        Status::Down
    } else if (down + pending) * 2 >= beats.len() {
        Status::Pending
    } else {
        Status::Up
    };
    let mut up: Vec<u32> = beats
        .iter()
        .filter(|b| b.status == Status::Up)
        .filter_map(|b| b.latency_ms)
        .collect();
    up.sort_unstable();
    MonitorStatus {
        monitor: monitor.id,
        time_ms: scheduled + 2_000,
        status,
        latency_ms: (status == Status::Up)
            .then(|| up.get(up.len() / 2).copied())
            .flatten(),
    }
}

const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

/// SplitMix64 finalizer.
fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A random-access draw keyed by the seed and `parts`.
fn draw<const N: usize>(seed: u64, parts: [u64; N]) -> u64 {
    parts
        .into_iter()
        .fold(mix64(seed), |h, part| mix64(h.wrapping_add(GAMMA) ^ part))
}

/// `floor(u * n / 2^64)`: uniform in `[0, n)`.
fn below(u: u64, n: u64) -> u64 {
    ((u as u128 * n as u128) >> 64) as u64
}

/// A standard normal value from two draws (Box-Muller).
fn normal(a: u64, b: u64) -> f64 {
    let unit = |u: u64| ((u >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    (-2.0 * unit(a).ln()).sqrt() * (std::f64::consts::TAU * unit(b)).cos()
}
