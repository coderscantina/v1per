//! Storage gate commands. See docs/evidence/m02-storage-gate.md for the run order.
//!
//! - `prefill`: generated history through `admit` and `record_statuses` in hour-sized
//!   admissions, then drained into DuckDB.
//! - `live`: the release-gate rate in real time through the real agent and evaluator
//!   admission path, with one retention run and status page reads in the middle.
//! - `queries`: the release-gate query suite, cold (fresh open per sample) and warm.
//! - `layout`: rows, bytes and file sizes per table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use v1per_storage::{
    Admitted, BatchId, Budgets, HEARTBEATS_FILE, INBOX_FILE, MonitorId, ProbeId, Read, ReadError,
    Refusal, Resolution, Retention, Series, Storage, UptimeWindow,
};
use v1per_workload::{DAY_MS, HOUR_MS, MINUTE_MS, Manifest, Workload};

/// The run state kept next to the data: which workload filled it and up to when.
const STATE_FILE: &str = "workload.json";

#[derive(Parser)]
struct Cli {
    /// Data directory (inbox.sqlite, heartbeats.duckdb).
    #[arg(long)]
    data: PathBuf,
    /// Free bytes the data volume keeps, in MiB. The reference default is 4096.
    #[arg(long, default_value_t = 4096)]
    disk_reserve_mib: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fill `days` of history ending at `until` (default: now, floored to the hour).
    Prefill {
        #[arg(long, default_value_t = 91)]
        days: i64,
        #[arg(long)]
        until: Option<DateTime<Utc>>,
    },
    /// Run the release-gate rate in real time for `minutes`.
    Live {
        #[arg(long, default_value_t = 10)]
        minutes: u64,
        /// Start raw retention, then minute-rollup retention at 90 days, this far into the run.
        #[arg(long)]
        expire_after_s: Option<u64>,
        /// Monitors on every agent multiply by this, for headroom runs.
        #[arg(long, default_value_t = 1)]
        scale: u32,
    },
    /// Release-gate query suite.
    Queries {
        #[arg(long, default_value_t = 50)]
        warm: usize,
        #[arg(long, default_value_t = 10)]
        cold: usize,
    },
    /// Row counts, bytes per table and file sizes.
    Layout,
    /// DuckDB's profile of each query case, one text file per case in `out`.
    Plans {
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct State {
    manifest: Manifest,
    from_ms: i64,
    /// End of the newest generated data.
    data_end_ms: i64,
}

impl State {
    fn read(data: &Path) -> State {
        let text = std::fs::read_to_string(data.join(STATE_FILE)).expect("run prefill first");
        serde_json::from_str(&text).expect("workload.json")
    }

    fn write(&self, data: &Path) {
        std::fs::write(
            data.join(STATE_FILE),
            serde_json::to_string_pretty(self).unwrap(),
        )
        .expect("write workload.json");
    }
}

/// The reference host budget: 2 DuckDB threads and 1 GiB, as in holocron's 0008.
fn budgets(disk_reserve_mib: u64) -> Budgets {
    Budgets {
        disk_reserve_bytes: disk_reserve_mib * 1024 * 1024,
        ..Budgets::default()
    }
}

// Two Tokio workers, as the server runs on the 2 vCPU reference host.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.data).expect("data directory");
    let budgets = budgets(cli.disk_reserve_mib);
    let report = match cli.command {
        Command::Prefill { days, until } => prefill(&cli.data, budgets, days, until).await,
        Command::Live {
            minutes,
            expire_after_s,
            scale,
        } => live(&cli.data, budgets, minutes, expire_after_s, scale).await,
        Command::Queries { warm, cold } => queries(&cli.data, budgets, warm, cold).await,
        Command::Layout => layout(&cli.data, budgets).await,
        Command::Plans { out } => plans(&cli.data, budgets, &out).await,
    };
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

async fn prefill(data: &Path, budgets: Budgets, days: i64, until: Option<DateTime<Utc>>) -> Json {
    let until_ms = until.map_or_else(
        || Utc::now().timestamp_millis().div_euclid(HOUR_MS) * HOUR_MS,
        |t| t.timestamp_millis(),
    );
    let from_ms = until_ms - days * DAY_MS;
    let manifest = Manifest::default();
    let storage = Storage::open(data, budgets).await.expect("open storage");
    let started = Instant::now();

    // Generation runs on its own thread, one hour ahead of admission.
    let (windows, mut ready) = tokio::sync::mpsc::channel(2);
    let generator = Workload::new(manifest.clone());
    std::thread::spawn(move || {
        for hour in (from_ms..until_ms).step_by(HOUR_MS as usize) {
            if windows
                .blocking_send((hour, generator.generate(hour, hour + HOUR_MS)))
                .is_err()
            {
                return;
            }
        }
    });
    let (mut heartbeats, mut statuses) = (0u64, 0u64);
    while let Some((hour, window)) = ready.recv().await {
        retry(|| storage.admit(None, &window.heartbeats)).await;
        retry(|| storage.record_statuses(&window.statuses)).await;
        heartbeats += window.heartbeats.len() as u64;
        statuses += window.statuses.len() as u64;
        if (hour + HOUR_MS - from_ms) % DAY_MS == 0 {
            let health = storage.health();
            eprintln!(
                "prefill: day {} of {days}, {:.0} s, {heartbeats} heartbeats, inbox {} rows, applied {} batches, apply {:.1} s, rollups {:.1} s",
                (hour + HOUR_MS - from_ms) / DAY_MS,
                started.elapsed().as_secs_f64(),
                health.inbox_rows,
                health.applied_batches,
                health.apply_time.as_secs_f64(),
                health.rollup_time.as_secs_f64(),
            );
        }
    }
    let admitted = started.elapsed();
    storage.drain().await;
    let drained = started.elapsed();
    let health = storage.health();
    storage.close().await.expect("close");
    State {
        manifest: manifest.clone(),
        from_ms,
        data_end_ms: until_ms,
    }
    .write(data);
    json!({
        "command": "prefill",
        "manifest": manifest,
        "from": iso(from_ms),
        "until": iso(until_ms),
        "heartbeats": heartbeats,
        "statuses": statuses,
        "admitted_s": admitted.as_secs_f64(),
        "drained_s": drained.as_secs_f64(),
        "entries_per_s": (heartbeats + statuses) as f64 / drained.as_secs_f64(),
        "transfer_batches": health.applied_batches,
        "apply_s": health.apply_time.as_secs_f64(),
        "rollup_s": health.rollup_time.as_secs_f64(),
        "peak_rss_mib": peak_rss_mib(),
    })
}

/// Retries an admission refused as busy; anything else is a failed run.
async fn retry<F, Fut>(admit: F) -> Admitted
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Admitted, Refusal>>,
{
    loop {
        match admit().await {
            Ok(admitted) => return admitted,
            Err(Refusal::Busy) => tokio::time::sleep(Duration::from_millis(100)).await,
            Err(other) => panic!("admission refused: {other}"),
        }
    }
}

/// One admission the live run sends at `send_ms`.
enum Job {
    Agent {
        probe: ProbeId,
        window_ms: i64,
        heartbeats: Vec<v1per_storage::Heartbeat>,
    },
    Evaluator(Vec<v1per_storage::MonitorStatus>),
}

#[derive(Default)]
struct LiveSamples {
    /// (sent at, ms since start; admission latency ms; entries)
    agent: Vec<(f64, f64, usize)>,
    evaluator: Vec<(f64, f64, usize)>,
    /// Acknowledgement to visible in a read, ms.
    visibility: Vec<f64>,
    /// Per transfer batch: apply ms, rollup ms.
    transfer: Vec<(f64, f64)>,
    page_reads: Vec<(f64, f64)>,
    rss_mib: Vec<f64>,
    inbox_age_ms: Vec<f64>,
    refused: u64,
}

async fn live(
    data: &Path,
    budgets: Budgets,
    minutes: u64,
    expire_after_s: Option<u64>,
    scale: u32,
) -> Json {
    let mut state = State::read(data);
    let manifest = Manifest {
        fast_monitors: state.manifest.fast_monitors * scale,
        slow_monitors: state.manifest.slow_monitors * scale,
        ..state.manifest.clone()
    };
    let workload = Workload::new(manifest.clone());
    let storage = Arc::new(Storage::open(data, budgets).await.expect("open storage"));
    let batch_ms = i64::from(manifest.agent_batch_s) * 1000;
    let eval_ms = i64::from(manifest.evaluator_batch_s) * 1000;
    let probes = i64::from(manifest.probes);
    let start_ms = (now_ms() / batch_ms + 1) * batch_ms;
    let end_ms = start_ms + minutes as i64 * MINUTE_MS;
    let clock = Instant::now() + Duration::from_millis((start_ms - now_ms()).max(0) as u64);
    let at = move |ms: i64| clock + Duration::from_millis((ms - start_ms).max(0) as u64);
    let since_start = move || clock.elapsed().as_secs_f64();
    let samples = Arc::new(Mutex::new(LiveSamples::default()));
    let acked: Arc<Mutex<BTreeMap<i64, Instant>>> = Arc::default();
    let done = Arc::new(tokio::sync::Notify::new());

    // Visibility and per-batch transfer cost, from watermark changes.
    let watcher = {
        let (storage, samples, acked) = (storage.clone(), samples.clone(), acked.clone());
        tokio::spawn(async move {
            let mut watermark = storage.watermark();
            let mut last = storage.health();
            while watermark.changed().await.is_ok() {
                let sequence = watermark.borrow_and_update().sequence;
                let now = Instant::now();
                let health = storage.health();
                let batches = health.applied_batches - last.applied_batches;
                let mut samples = samples.lock().unwrap();
                if batches > 0 {
                    let per = |d: Duration| d.as_secs_f64() * 1000.0 / batches as f64;
                    samples.transfer.push((
                        per(health.apply_time - last.apply_time),
                        per(health.rollup_time - last.rollup_time),
                    ));
                }
                last = health;
                let mut acked = acked.lock().unwrap();
                let rest = acked.split_off(&(sequence + 1));
                for (_, ack) in std::mem::replace(&mut *acked, rest) {
                    samples.visibility.push(ms(now - ack));
                }
            }
        })
    };

    // Status page reads under load, RSS and inbox age.
    let sampler = {
        let (storage, samples, done) = (storage.clone(), samples.clone(), done.clone());
        let all: Vec<MonitorId> = workload.monitors().iter().map(|m| m.id).collect();
        let page = page_components(&all);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = done.notified() => return,
                }
                let now = now_ms();
                let today = local_day(now, chrono_tz::Europe::Vienna);
                let started = Instant::now();
                storage
                    .day_bars(&page, Series::Monitor, chrono_tz::Europe::Vienna, today, 90)
                    .await
                    .expect("page read");
                let bars = ms(started.elapsed());
                let started = Instant::now();
                storage
                    .latest(&all, now - 5 * MINUTE_MS)
                    .await
                    .expect("latest read");
                let latest = ms(started.elapsed());
                let age = storage.inbox_age().await.expect("inbox age");
                let mut samples = samples.lock().unwrap();
                samples.page_reads.push((bars, latest));
                samples.rss_mib.push(rss_mib());
                samples.inbox_age_ms.push(ms(age));
            }
        })
    };

    // Retention in the middle of the run: raw first, then minute rollups at 90 days.
    let expiry = expire_after_s.map(|after| {
        let storage = storage.clone();
        tokio::spawn(async move {
            tokio::time::sleep_until((clock + Duration::from_secs(after)).into()).await;
            let mut runs = Vec::new();
            for minute in [None, Some(Duration::from_secs(90 * 86_400))] {
                let from = since_start();
                let expired = storage
                    .expire(
                        Retention {
                            minute_rollups: minute,
                            ..Retention::default()
                        },
                        now_ms(),
                    )
                    .await
                    .expect("expire");
                runs.push(json!({
                    "start_s": from,
                    "end_s": since_start(),
                    "heartbeats": expired.heartbeats,
                    "minute_rollups": expired.minute_rollups,
                }));
            }
            runs
        })
    });

    // The send schedule, built one agent window ahead.
    let mut schedule: BTreeMap<(i64, u64), Job> = BTreeMap::new();
    let mut order = 0u64;
    let mut next_window = start_ms - batch_ms;
    let mut tasks = Vec::new();
    loop {
        while next_window < end_ms && next_window <= now_ms() + batch_ms {
            let window = workload.generate(next_window, next_window + batch_ms);
            let mut by_probe: BTreeMap<ProbeId, Vec<_>> = BTreeMap::new();
            for beat in window.heartbeats {
                by_probe
                    .entry(beat.probe.expect("probe"))
                    .or_default()
                    .push(beat);
            }
            // Agents are not synchronized: each sends at its own offset after the window.
            for (probe, heartbeats) in by_probe {
                let send = next_window + batch_ms + (probe.0 - 1) * batch_ms / probes;
                order += 1;
                schedule.insert(
                    (send, order),
                    Job::Agent {
                        probe,
                        window_ms: next_window,
                        heartbeats,
                    },
                );
            }
            let mut by_second: BTreeMap<i64, Vec<_>> = BTreeMap::new();
            for status in window.statuses {
                by_second
                    .entry(status.time_ms.div_euclid(eval_ms) * eval_ms + eval_ms)
                    .or_default()
                    .push(status);
            }
            for (send, statuses) in by_second {
                order += 1;
                schedule.insert((send.max(start_ms), order), Job::Evaluator(statuses));
            }
            next_window += batch_ms;
        }
        let Some((&(send, key), _)) = schedule.first_key_value() else {
            break;
        };
        if send >= end_ms {
            break;
        }
        let job = schedule.remove(&(send, key)).unwrap();
        tokio::time::sleep_until(at(send).into()).await;
        let (storage, samples, acked) = (storage.clone(), samples.clone(), acked.clone());
        tasks.push(tokio::spawn(async move {
            let sent = since_start();
            let started = Instant::now();
            let (outcome, entries, agent) = match &job {
                Job::Agent {
                    probe,
                    window_ms,
                    heartbeats,
                } => {
                    let batch = BatchId {
                        probe: *probe,
                        id: format!("{}-{window_ms}", probe.0),
                    };
                    (
                        storage.admit(Some(batch), heartbeats).await,
                        heartbeats.len(),
                        true,
                    )
                }
                Job::Evaluator(statuses) => (
                    storage.record_statuses(statuses).await,
                    statuses.len(),
                    false,
                ),
            };
            let latency = ms(started.elapsed());
            let acknowledged = Instant::now();
            let mut samples = samples.lock().unwrap();
            match outcome {
                Ok(admitted) => {
                    acked
                        .lock()
                        .unwrap()
                        .insert(admitted.sequence, acknowledged);
                    let list = if agent {
                        &mut samples.agent
                    } else {
                        &mut samples.evaluator
                    };
                    list.push((sent, latency, entries));
                }
                Err(_) => samples.refused += 1,
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let sent_s = since_start();
    storage.drain().await;
    let drained_s = since_start();
    done.notify_one();
    sampler.await.unwrap();
    let expiry = match expiry {
        Some(task) => Some(task.await.unwrap()),
        None => None,
    };
    // Let the watcher see the last watermark before reading the samples.
    tokio::time::sleep(Duration::from_millis(200)).await;
    watcher.abort();
    let health = storage.health();
    storage.shutdown().await.expect("shutdown");
    if scale == 1 {
        state.data_end_ms = state.data_end_ms.max(end_ms);
        state.write(data);
    }

    let samples = samples.lock().unwrap();
    let window = |expired: Option<&Vec<Json>>| {
        expired.map(|runs| {
            let from = runs.first().unwrap()["start_s"].as_f64().unwrap();
            let to = runs.last().unwrap()["end_s"].as_f64().unwrap();
            (from, to)
        })
    };
    let during = window(expiry.as_ref());
    let agent = |inside: Option<bool>| {
        let values: Vec<f64> = samples
            .agent
            .iter()
            .filter(|(sent, _, _)| match (inside, during) {
                (Some(want), Some((from, to))) => (from..=to).contains(sent) == want,
                _ => true,
            })
            .map(|s| s.1)
            .collect();
        stats(&values)
    };
    json!({
        "command": "live",
        "scale": scale,
        "minutes": minutes,
        "monitors": workload.monitors().len(),
        "agent_batches": samples.agent.len(),
        "agent_entries": samples.agent.iter().map(|s| s.2).sum::<usize>(),
        "agent_batch_entries": stats(&samples.agent.iter().map(|s| s.2 as f64).collect::<Vec<_>>()),
        "evaluator_batches": samples.evaluator.len(),
        "refused": samples.refused,
        "admission_agent_ms": agent(None),
        "admission_agent_during_expiry_ms": during.map(|_| agent(Some(true))),
        "admission_agent_outside_expiry_ms": during.map(|_| agent(Some(false))),
        "admission_evaluator_ms": stats(&samples.evaluator.iter().map(|s| s.1).collect::<Vec<_>>()),
        "visibility_ms": stats(&samples.visibility),
        "transfer_apply_ms": stats(&samples.transfer.iter().map(|s| s.0).collect::<Vec<_>>()),
        "transfer_rollup_ms": stats(&samples.transfer.iter().map(|s| s.1).collect::<Vec<_>>()),
        "transfer_batches": samples.transfer.len(),
        "inbox_age_ms": stats(&samples.inbox_age_ms),
        "page_bars_vienna_50_under_load_ms": stats(&samples.page_reads.iter().map(|s| s.0).collect::<Vec<_>>()),
        "latest_2000_under_load_ms": stats(&samples.page_reads.iter().map(|s| s.1).collect::<Vec<_>>()),
        "rss_mib": stats(&samples.rss_mib),
        "peak_rss_mib": peak_rss_mib(),
        "expiry": expiry,
        "sent_s": sent_s,
        "drained_s": drained_s,
        "transfer_failures": health.transfer_failures,
    })
}

/// A status page of 50 components spread over the monitor IDs.
fn page_components(all: &[MonitorId]) -> Vec<MonitorId> {
    all.iter()
        .step_by((all.len() / 50).max(1))
        .take(50)
        .copied()
        .collect()
}

fn local_day(ms: i64, zone: Tz) -> chrono::NaiveDate {
    DateTime::from_timestamp_millis(ms)
        .unwrap()
        .with_timezone(&zone)
        .date_naive()
}

#[derive(Clone, Copy)]
enum Query {
    Latest,
    Bars(Tz),
    Latency,
    Uptime(UptimeWindow, usize),
}

const CASES: [(&str, Query); 12] = [
    ("latest-2000", Query::Latest),
    ("bars-90d-50-vienna", Query::Bars(chrono_tz::Europe::Vienna)),
    ("bars-90d-50-kolkata", Query::Bars(chrono_tz::Asia::Kolkata)),
    ("latency-30d-1", Query::Latency),
    ("uptime-24h-1", Query::Uptime(UptimeWindow::Day, 1)),
    ("uptime-24h-50", Query::Uptime(UptimeWindow::Day, 50)),
    ("uptime-7d-1", Query::Uptime(UptimeWindow::Week, 1)),
    ("uptime-7d-50", Query::Uptime(UptimeWindow::Week, 50)),
    ("uptime-30d-1", Query::Uptime(UptimeWindow::Month, 1)),
    ("uptime-30d-50", Query::Uptime(UptimeWindow::Month, 50)),
    ("uptime-1y-1", Query::Uptime(UptimeWindow::Year, 1)),
    ("uptime-1y-50", Query::Uptime(UptimeWindow::Year, 50)),
];

/// What the cases read: every monitor, one page of 50, and the end of the data.
struct Inputs {
    all: Vec<MonitorId>,
    page: Vec<MonitorId>,
    now: i64,
}

/// Runs one case; returns queue wait, execution and the rows (monitors, bars, points) read.
async fn run(storage: &Storage, query: Query, inputs: &Inputs) -> (Duration, Duration, usize) {
    fn timed<T>(
        read: Result<Read<T>, ReadError>,
        size: impl Fn(&T) -> usize,
    ) -> (Duration, Duration, usize) {
        let read = read.expect("read");
        (read.queue_wait, read.execution, size(&read.value))
    }
    let Inputs { all, page, now } = inputs;
    let now = *now;
    match query {
        Query::Latest => timed(storage.latest(all, now - 5 * MINUTE_MS).await, Vec::len),
        Query::Bars(zone) => timed(
            storage
                .day_bars(page, Series::Monitor, zone, local_day(now, zone), 90)
                .await,
            |bars| bars.values().map(Vec::len).sum(),
        ),
        Query::Latency => timed(
            storage
                .latency(
                    page[0],
                    Series::Monitor,
                    Resolution::Hour,
                    now - 30 * DAY_MS,
                    now,
                )
                .await,
            Vec::len,
        ),
        Query::Uptime(window, monitors) => timed(
            storage
                .uptime(&page[..monitors], Series::Monitor, window, now)
                .await,
            BTreeMap::len,
        ),
    }
}

async fn queries(data: &Path, budgets: Budgets, warm: usize, cold: usize) -> Json {
    let state = State::read(data);
    let all: Vec<MonitorId> = Workload::new(state.manifest.clone())
        .monitors()
        .iter()
        .map(|m| m.id)
        .collect();
    let inputs = Inputs {
        page: page_components(&all),
        all,
        now: state.data_end_ms,
    };
    // Cold: a fresh open per sample, so DuckDB's buffer pool starts empty. The OS page cache
    // stays warm; clearing it needs root.
    let mut cold_samples: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
    for _ in 0..cold {
        for (id, query) in CASES {
            let storage = Storage::open(data, budgets.clone()).await.expect("open");
            let (wait, exec, _) = run(&storage, query, &inputs).await;
            cold_samples.entry(id).or_default().push(ms(wait + exec));
            storage.close().await.expect("close");
        }
    }
    let storage = Storage::open(data, budgets).await.expect("open");
    let mut report = serde_json::Map::new();
    for (id, query) in CASES {
        let (_, _, rows) = run(&storage, query, &inputs).await;
        let (mut total, mut execution) = (Vec::new(), Vec::new());
        for _ in 0..warm {
            let (wait, exec, _) = run(&storage, query, &inputs).await;
            total.push(ms(wait + exec));
            execution.push(ms(exec));
        }
        report.insert(
            id.to_owned(),
            json!({
                "rows": rows,
                "cold_ms": stats(cold_samples.get(id).map_or(&[][..], Vec::as_slice)),
                "warm_ms": stats(&total),
                "warm_execution_ms": stats(&execution),
            }),
        );
    }
    storage.close().await.expect("close");
    json!({
        "command": "queries",
        "now": iso(state.data_end_ms),
        "cases": report,
        "peak_rss_mib": peak_rss_mib(),
    })
}

/// `EXPLAIN ANALYZE` of the rollup and raw scans behind the slowest cases, written as
/// `crates/storage/src/queries.rs` emits them for the monitor series without imported rows.
async fn plans(data: &Path, budgets: Budgets, out: &Path) -> Json {
    std::fs::create_dir_all(out).expect("plans directory");
    let state = State::read(data);
    let all: Vec<MonitorId> = Workload::new(state.manifest.clone())
        .monitors()
        .iter()
        .map(|m| m.id)
        .collect();
    let ids = |list: &[MonitorId]| {
        list.iter()
            .map(|m| m.0.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    let (page, every) = (ids(&page_components(&all)), ids(&all));
    let now = state.data_end_ms;
    let at = |ms: i64| format!("make_timestamp({}::BIGINT * 1000)", ms);
    let rollup = |table: &str, from: i64, to: i64, bucket: &str| {
        format!(
            "SELECT monitor_id, {bucket} AS b, sum(up), sum(down), sum(pending), sum(maintenance),
                    min(latency_min), max(latency_max), sum(latency_sum), sum(latency_count)
             FROM {table} WHERE monitor_id IN ({page}) AND probe_id IS NULL
               AND bucket >= {} AND bucket < {}
             GROUP BY monitor_id, b ORDER BY monitor_id, b",
            at(from),
            at(to)
        )
    };
    let day_end = now.div_euclid(DAY_MS) * DAY_MS + DAY_MS;
    let minute_end = now.div_euclid(MINUTE_MS) * MINUTE_MS + MINUTE_MS;
    let plans = [
        (
            "bars-90d-50",
            rollup("rollup_hour", day_end - 92 * DAY_MS, day_end + DAY_MS, "epoch_ms(bucket)"),
        ),
        (
            "uptime-24h-50",
            rollup("rollup_minute", minute_end - DAY_MS, minute_end, "0"),
        ),
        (
            "latest-2000",
            format!(
                "SELECT monitor_id, probe_id, epoch_ms(time), status, latency_ms, message, retries,
                        http_status, epoch_ms(cert_expiry)
                 FROM heartbeat WHERE monitor_id IN ({every}) AND time >= {}
                 QUALIFY row_number() OVER (PARTITION BY monitor_id ORDER BY time DESC, probe_id) = 1
                 ORDER BY monitor_id",
                at(now - 5 * MINUTE_MS)
            ),
        ),
    ];
    // Experiment: the same bars read over a temporary copy of the hour rollups clustered by
    // series, which lets zone maps skip other monitors.
    let clustered = rollup(
        "temp.rollup_hour_clustered",
        day_end - 92 * DAY_MS,
        day_end + DAY_MS,
        "epoch_ms(bucket)",
    );
    let plans = plans
        .into_iter()
        .chain([("bars-90d-50-clustered", clustered)]);
    let storage = Storage::open(
        data,
        Budgets {
            readers: 1,
            ..budgets
        },
    )
    .await
    .expect("open");
    storage
        .read(|s| {
            s.connection().execute_batch(
                "CREATE TEMP TABLE rollup_hour_clustered AS
                 SELECT * FROM rollup_hour ORDER BY probe_id NULLS FIRST, monitor_id, bucket",
            )
        })
        .await
        .expect("clustered copy");
    let mut files = Vec::new();
    for (id, sql) in plans {
        let text = storage
            .read(move |s| {
                // Warm the buffer pool first, so the plan shows the warm case.
                s.connection().execute_batch(&sql)?;
                s.connection()
                    .query_row(&format!("EXPLAIN ANALYZE {sql}"), [], |r| {
                        r.get::<_, String>(1)
                    })
            })
            .await
            .expect("explain")
            .value;
        let file = out.join(format!("{id}.txt"));
        std::fs::write(&file, text).expect("write plan");
        files.push(file.display().to_string());
    }
    storage.close().await.expect("close");
    json!({ "command": "plans", "files": files })
}

async fn layout(data: &Path, budgets: Budgets) -> Json {
    let state = State::read(data);
    let storage = Storage::open(data, budgets).await.expect("open");
    let tables = storage
        .read(|s| {
            let conn = s.connection();
            let block: i64 = conn.query_row(
                "SELECT block_size FROM pragma_database_size()",
                [],
                |r| r.get(0),
            )?;
            let mut tables = Vec::new();
            for (table, column) in [
                ("heartbeat", "time"),
                ("rollup_minute", "bucket"),
                ("rollup_hour", "bucket"),
                ("rollup_day", "bucket"),
                ("applied_batches", "applied_at"),
            ] {
                let (rows, days): (i64, f64) = conn.query_row(
                    &format!(
                        "SELECT count(*), coalesce(epoch_ms(max({column}) - min({column})) / 86400000.0, 0) FROM {table}"
                    ),
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                let blocks: i64 = conn.query_row(
                    &format!(
                        "SELECT count(DISTINCT block_id) FROM pragma_storage_info('{table}') WHERE block_id >= 0"
                    ),
                    [],
                    |r| r.get(0),
                )?;
                // Rows of full UTC days, for per-day and per-year figures.
                let per_day: Option<f64> = conn.query_row(
                    &format!(
                        "SELECT median(n)::DOUBLE FROM (
                             SELECT count(*) AS n FROM {table} GROUP BY date_trunc('day', {column})
                         ) WHERE n > 0"
                    ),
                    [],
                    |r| r.get(0),
                )?;
                tables.push((table, rows, days, blocks * block, per_day));
            }
            let size: (i64, i64, i64) = conn.query_row(
                "SELECT block_size, used_blocks, free_blocks FROM pragma_database_size()",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            Ok((tables, size))
        })
        .await
        .expect("layout read")
        .value;
    storage.close().await.expect("close");
    let (tables, (block, used, free)) = tables;
    let file = |name: &str| {
        std::fs::metadata(data.join(name)).map_or(0, |m| m.len()) as f64 / 1024.0 / 1024.0
    };
    json!({
        "command": "layout",
        "from": iso(state.from_ms),
        "data_end": iso(state.data_end_ms),
        "heartbeats_duckdb_mib": file(HEARTBEATS_FILE),
        "inbox_sqlite_mib": file(INBOX_FILE),
        "duckdb_used_mib": (used * block) as f64 / 1024.0 / 1024.0,
        "duckdb_free_mib": (free * block) as f64 / 1024.0 / 1024.0,
        "tables": tables.iter().map(|(table, rows, days, bytes, per_day)| json!({
            "table": table,
            "rows": rows,
            "span_days": days,
            "mib": *bytes as f64 / 1024.0 / 1024.0,
            "bytes_per_row": if *rows > 0 { *bytes as f64 / *rows as f64 } else { 0.0 },
            "median_rows_per_utc_day": per_day,
            "rows_per_365_days": per_day.map(|d| d * 365.0),
        })).collect::<Vec<_>>(),
    })
}

fn stats(values: &[f64]) -> Json {
    if values.is_empty() {
        return json!({ "n": 0 });
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let pick =
        |q: f64| sorted[((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1];
    json!({
        "n": sorted.len(),
        "p50": round(pick(0.50)),
        "p95": round(pick(0.95)),
        "p99": round(pick(0.99)),
        "max": round(*sorted.last().unwrap()),
    })
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn iso(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms).unwrap().to_rfc3339()
}

/// Resident set size of this process, from `ps`.
fn rss_mib() -> f64 {
    std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map_or(0.0, |kib| kib / 1024.0)
}

/// Peak resident set size of this process.
fn peak_rss_mib() -> f64 {
    // SAFETY: getrusage fills the zeroed struct for RUSAGE_SELF.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    // macOS reports bytes, Linux KiB.
    let divisor = if cfg!(target_os = "macos") {
        1024.0 * 1024.0
    } else {
        1024.0
    };
    usage.ru_maxrss as f64 / divisor
}
