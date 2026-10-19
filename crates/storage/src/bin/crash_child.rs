//! Test support for `tests/failpoints.rs`, built only with the `failpoints` feature.
//!
//! `run <dir> <seed>` admits seeded heartbeat batches one at a time and logs each before
//! admission (`begun`) and after its acknowledgement (`acked`), fsyncing the log, then waits
//! for transfer to drain. The parent arms one failpoint, so the process is usually killed
//! partway.
//!
//! `verify <dir> <seed> <boundary>` reopens the store without failpoints, drains, and checks
//! every batch against the logs, the raw table, the rollups and the ledger. It then retries
//! every begun agent batch, as an agent without an acknowledgement does, and checks that
//! each is stored exactly once. It prints `ok <summary>` or exits non-zero with the violation.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use v1per_storage::{
    BatchId, Budgets, Heartbeat, MonitorId, MonitorStatus, ProbeId, Status, Storage,
};

const DAY_MS: i64 = 86_400_000;
const NOW_MS: i64 = 1_790_812_800_000; // 2026-10-01T00:00:00Z

fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        ((mix64(self.0) as u128 * n as u128) >> 64) as u64
    }
}

/// One seeded admission. Every heartbeat has a distinct time, which identifies it.
struct Request {
    batch: Option<BatchId>,
    heartbeats: Vec<Heartbeat>,
    /// Evaluator entries, each request on its own monitor from `STATUS_MONITORS` on, since
    /// they have no raw row to identify them by.
    statuses: Vec<MonitorStatus>,
}

const STATUS_MONITORS: i64 = 1000;

fn budgets(seed: u64) -> Budgets {
    Budgets {
        batch_rows: 1 + (seed % 4) as usize,
        transfer_delay: Duration::from_millis(2),
        engine_threads: 1,
        engine_memory_bytes: 256 * 1024 * 1024,
        disk_reserve_bytes: 0,
        ..Budgets::default()
    }
}

fn requests(seed: u64) -> Vec<Request> {
    let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D) ^ 0x0056_3150_4552);
    let count = 12 + rng.below(8);
    let mut next = 0i64;
    (0..count)
        .map(|index| {
            let size = 1 + rng.below(25);
            if index % 5 == 3 {
                let statuses = (0..size)
                    .map(|_| {
                        next += 1;
                        MonitorStatus {
                            monitor: MonitorId(STATUS_MONITORS + index as i64),
                            time_ms: NOW_MS + next * 1000,
                            status: Status::Up,
                            latency_ms: Some(rng.below(2000) as u32),
                        }
                    })
                    .collect();
                return Request {
                    batch: None,
                    heartbeats: Vec::new(),
                    statuses,
                };
            }
            let heartbeats = (0..size)
                .map(|_| {
                    next += 1;
                    // One in eight is late: an agent flushing a buffer up to 30 days old.
                    let late = if rng.below(8) == 0 {
                        rng.below(30) as i64 * DAY_MS
                    } else {
                        0
                    };
                    let status = [
                        Status::Down,
                        Status::Up,
                        Status::Pending,
                        Status::Maintenance,
                    ][rng.below(4) as usize];
                    Heartbeat {
                        monitor: MonitorId(1 + rng.below(5) as i64),
                        // Built-in admissions stand in for server-decided monitors.
                        probe: (index % 4 != 0).then(|| ProbeId(1 + (index % 3) as i64)),
                        time_ms: NOW_MS + next * 1000 - late,
                        status,
                        latency_ms: (status == Status::Up).then(|| rng.below(2000) as u32),
                        message: "ok".into(),
                        retries: 0,
                        http_status: Some(200),
                        cert_expiry_ms: None,
                    }
                })
                .collect();
            // Every fourth admission comes from the built-in probe, without a batch ID.
            let batch = (index % 4 != 0).then(|| BatchId {
                probe: ProbeId(1 + (index % 3) as i64),
                id: format!("{seed}-{index}"),
            });
            Request {
                batch,
                heartbeats,
                statuses: Vec::new(),
            }
        })
        .collect()
}

fn log(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open log");
    writeln!(file, "{line}").expect("write log");
    file.sync_data().expect("sync log");
}

fn read_log(path: &Path) -> HashSet<usize> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| line.parse().expect("log line"))
        .collect()
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = Path::new(&args[2]);
    let seed: u64 = args[3].parse().expect("seed");
    match args[1].as_str() {
        "run" => run(dir, seed).await,
        "verify" => verify(dir, seed, &args[4]).await,
        other => panic!("unknown mode {other}"),
    }
}

async fn run(dir: &Path, seed: u64) {
    let storage = Storage::open(dir, budgets(seed)).await.expect("open");
    for (index, request) in requests(seed).into_iter().enumerate() {
        log(&dir.join("begun.log"), &index.to_string());
        let admitted = if request.statuses.is_empty() {
            storage.admit(request.batch, &request.heartbeats).await
        } else {
            storage.record_statuses(&request.statuses).await
        }
        .expect("admit");
        assert!(!admitted.repeated);
        log(&dir.join("acked.log"), &index.to_string());
    }
    storage.drain().await;
    storage.close().await.expect("close");
}

/// What a run left behind.
struct Stored {
    /// Copies of each raw heartbeat, by time.
    copies: HashMap<i64, i64>,
    /// Rollup rows that disagree with the raw rows, outside the status monitors.
    mismatched: i64,
    /// Entries counted per status monitor in the minute, hour and day rollups.
    statuses: HashMap<i64, [i64; 3]>,
    ledger: Vec<[i64; 4]>,
}

async fn stored(storage: &Storage) -> Stored {
    storage
        .read(|snapshot| {
            let conn = snapshot.connection();
            let copies = conn
                .prepare("SELECT epoch_ms(time), count(*) FROM heartbeat GROUP BY 1")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<duckdb::Result<HashMap<i64, i64>>>()?;
            let mut mismatched = 0i64;
            for (table, unit) in [
                ("rollup_minute", "minute"),
                ("rollup_hour", "hour"),
                ("rollup_day", "day"),
            ] {
                mismatched += conn.query_row(
                    &format!(
                        "WITH raw AS (
                             SELECT monitor_id, probe_id, date_trunc('{unit}', time)::TIMESTAMP AS bucket,
                                    count(*) FILTER (WHERE status = 1) AS up,
                                    count(*) FILTER (WHERE status = 0) AS down,
                                    count(*) FILTER (WHERE status = 2) AS pending,
                                    count(*) FILTER (WHERE status = 3) AS maintenance,
                                    coalesce(sum(latency_ms) FILTER (WHERE status = 1), 0) AS latency_sum
                             FROM heartbeat GROUP BY ALL)
                         SELECT count(*) FROM raw
                         FULL OUTER JOIN (SELECT * FROM {table} WHERE monitor_id < {STATUS_MONITORS}) AS r
                           ON raw.monitor_id = r.monitor_id AND raw.bucket = r.bucket
                          AND raw.probe_id IS NOT DISTINCT FROM r.probe_id
                         WHERE raw.up IS DISTINCT FROM r.up OR raw.down IS DISTINCT FROM r.down
                            OR raw.pending IS DISTINCT FROM r.pending
                            OR raw.maintenance IS DISTINCT FROM r.maintenance
                            OR raw.latency_sum IS DISTINCT FROM r.latency_sum"
                    ),
                    [],
                    |r| r.get::<_, i64>(0),
                )?;
            }
            let ledger = conn
                .prepare(
                    "SELECT batch_id, first_seq, last_seq, records FROM applied_batches ORDER BY batch_id",
                )?
                .query_map([], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?]))?
                .collect::<duckdb::Result<Vec<[i64; 4]>>>()?;
            let statuses = conn
                .prepare(&format!(
                    "SELECT monitor_id,
                            (SELECT coalesce(sum(up + down + pending + maintenance), 0)::BIGINT FROM rollup_minute r WHERE r.monitor_id = m.monitor_id),
                            (SELECT coalesce(sum(up + down + pending + maintenance), 0)::BIGINT FROM rollup_hour r WHERE r.monitor_id = m.monitor_id),
                            (SELECT coalesce(sum(up + down + pending + maintenance), 0)::BIGINT FROM rollup_day r WHERE r.monitor_id = m.monitor_id)
                     FROM (SELECT DISTINCT monitor_id FROM rollup_minute WHERE monitor_id >= {STATUS_MONITORS}) AS m"
                ))?
                .query_map([], |r| Ok((r.get(0)?, [r.get(1)?, r.get(2)?, r.get(3)?])))?
                .collect::<duckdb::Result<HashMap<i64, [i64; 3]>>>()?;
            Ok(Stored {
                copies,
                mismatched,
                statuses,
                ledger,
            })
        })
        .await
        .expect("read")
        .value
}

async fn verify(dir: &Path, seed: u64, boundary: &str) {
    let begun = read_log(&dir.join("begun.log"));
    let acked = read_log(&dir.join("acked.log"));
    let requests = requests(seed);
    let storage = Storage::open(dir, budgets(seed)).await.expect("reopen");
    storage.drain().await;
    let Stored {
        copies,
        mismatched,
        statuses,
        ledger,
    } = stored(&storage).await;
    let health = storage.health();

    let mut failures = Vec::new();
    let mut stored_requests = 0;
    for (index, request) in requests.iter().enumerate() {
        let present: Vec<i64> = if request.statuses.is_empty() {
            request
                .heartbeats
                .iter()
                .map(|h| copies.get(&h.time_ms).copied().unwrap_or(0))
                .collect()
        } else {
            // Each resolution must count every entry the same whole number of times.
            let n = request.statuses.len() as i64;
            let counted = statuses
                .get(&(STATUS_MONITORS + index as i64))
                .copied()
                .unwrap_or_default();
            if counted.iter().any(|c| *c != counted[0] || c % n != 0) {
                failures.push(format!(
                    "request {index} statuses counted {counted:?} of {n}"
                ));
            }
            vec![counted[0] / n; n as usize]
        };
        let all = present.iter().all(|n| *n > 0);
        let none = present.iter().all(|n| *n == 0);
        stored_requests += usize::from(all);
        if !(all || none) {
            failures.push(format!("request {index} is partly stored"));
        }
        if present.iter().any(|n| *n > 1) {
            failures.push(format!("request {index} has duplicated heartbeats"));
        }
        if acked.contains(&index) && !all {
            failures.push(format!("acknowledged request {index} lost heartbeats"));
        }
        if !begun.contains(&index) && !none {
            failures.push(format!("request {index} was never admitted but is stored"));
        }
        let in_flight = begun.contains(&index) && !acked.contains(&index);
        if in_flight && boundary == "admission.before_commit" && !none {
            failures.push(format!("request {index} killed before commit is stored"));
        }
        if in_flight && boundary == "admission.after_commit" && !all {
            failures.push(format!(
                "request {index} committed before the kill is missing"
            ));
        }
    }
    if mismatched != 0 {
        failures.push(format!(
            "{mismatched} rollup rows disagree with raw heartbeats"
        ));
    }
    let ledger_rows: i64 = ledger.iter().map(|b| b[3]).sum();
    if ledger_rows as usize != stored_requests {
        failures.push(format!(
            "ledger counts {ledger_rows} admissions, {stored_requests} are stored"
        ));
    }
    for pair in ledger.windows(2) {
        if pair[1][0] <= pair[0][0] || pair[1][1] <= pair[0][2] {
            failures.push(format!(
                "ledger entries overlap or go backwards: {:?} {:?}",
                pair[0], pair[1]
            ));
        }
    }
    if let Some(last) = ledger.last()
        && (health.watermark.batch, health.watermark.sequence) != (last[0], last[2])
    {
        failures.push(format!(
            "watermark {:?} differs from ledger {last:?}",
            health.watermark
        ));
    }
    if health.inbox_rows != 0 {
        failures.push(format!("{} rows left in the inbox", health.inbox_rows));
    }

    // Agents retry every batch they began; a stored one must come back as a repeat.
    let mut retried = 0;
    for (index, request) in requests.iter().enumerate() {
        let Some(batch) = &request.batch else {
            continue;
        };
        if !begun.contains(&index) {
            continue;
        }
        let was_stored = copies.contains_key(&request.heartbeats[0].time_ms);
        let admitted = storage
            .admit(Some(batch.clone()), &request.heartbeats)
            .await
            .expect("retry");
        if admitted.repeated != was_stored {
            failures.push(format!(
                "retry of request {index}: repeated {} but stored {was_stored}",
                admitted.repeated
            ));
        }
        retried += 1;
    }
    storage.drain().await;
    let Stored {
        copies, mismatched, ..
    } = stored(&storage).await;
    for (index, request) in requests.iter().enumerate() {
        if request.batch.is_some()
            && begun.contains(&index)
            && request
                .heartbeats
                .iter()
                .any(|h| copies.get(&h.time_ms) != Some(&1))
        {
            failures.push(format!(
                "request {index} is not stored exactly once after retry"
            ));
        }
    }
    if mismatched != 0 {
        failures.push(format!(
            "{mismatched} rollup rows disagree with raw heartbeats after retry"
        ));
    }
    storage.close().await.expect("close");
    if failures.is_empty() {
        println!(
            "ok begun={} acked={} stored={stored_requests} retried={retried} batches={}",
            begun.len(),
            acked.len(),
            ledger.len()
        );
    } else {
        eprintln!("{}", failures.join("\n"));
        std::process::exit(1);
    }
}
