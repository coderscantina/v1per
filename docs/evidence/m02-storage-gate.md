# Milestone 02 storage gate evidence

Measured on 2026-10-18 by the storage owner. Budgets and the open minute rollup retention are in [0009](../decisions/0009-storage-budgets.md).

Verdict: every release-gate target the storage layer can answer holds on this machine with 91 days of generated data, at the full rate and at ten times the monitor count. This is not the reference host, so the gate is not passed until milestone 12 repeats it there.

## Machine and versions

| Item | Value |
| --- | --- |
| Machine | Apple M1 Pro laptop, 10 cores, 32 GiB RAM, internal SSD (Apple Fabric), macOS 27.0.1. Not the 2 vCPU, 4 GiB reference host |
| Limits | DuckDB 2 threads and 1 GiB (`Budgets::default()`), 2 Tokio workers, 2 read workers. Nothing caps the process at 2 cores or 4 GiB |
| Durability | SQLite WAL, `synchronous=FULL`, `fullfsync=ON` |
| Disk reserve | 1 GiB for these runs (the volume had 12 GiB free after the prefill); the server default stays 4 GiB |
| Toolchain | rustc 1.99.1, release profile (`debug = "line-tables-only"`, DuckDB without debug info) |
| Engines | DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 (libsqlite3-sys 0.37.0, SQLx 0.9.0) |
| Source | commit `c515916` plus the uncommitted milestone 02 work in `crates/storage` and `crates/workload` |
| OS page cache | never cleared (needs root); "cold" means a fresh `Storage::open` per sample, so DuckDB's buffer pool starts empty |

## Workload manifest

`v1per_workload::Manifest::default()`, seed `0x7631706572020001`. Every result is a pure function of the seed and the check, so any window can be generated alone and the prefill and live run produce the same data for the same window (`crates/workload/tests/workload.rs`).

| Item | Value |
| --- | --- |
| Monitors | 2,000: 400 at 20 s and 1,600 at 60 s, picked by seed; http 70 %, keyword 10 %, port 10 %, ping 5 %, dns 5 % |
| Probes | 10, probe 1 the built-in one. the other 90 % run on one probe (half of them on the built-in probe, the rest spread over 2 to 10); 9.6 % (191) run on the built-in probe plus two remote probes |
| Volume | 4,032,000 checks a day, 4,841,280 raw heartbeats a day, 4,032,000 evaluator statuses a day |
| Latency | log-normal per monitor (median 180 ms http, 25 ms port and ping, 20 ms dns), times a probe distance factor of 1.0 to 2.5, noise with sigma 0.25, 1 % spikes of 3 to 8 times |
| Failures | outage episodes (15 % chance per monitor and day, 1 to 180 minutes), probe partitions (30 % per probe and day, 2 to 15 minutes), 2 % flapping monitors (a fifth of their hours fail 40 % of checks), 0.05 % isolated failures, Kuma retries (half the monitors 0, 30 % 1, 20 % 3) giving pending beats |
| Maintenance | 1 % of monitors, Sunday 02:00 to 03:00 UTC |
| Evaluator | one monitor-level status per check: majority quorum (down when at least half the probes are down), pending when failing probes reach half, median latency of up probes |
| Arrival | each probe sends its results every 5 s as one agent batch with a batch ID, probes offset by 0.5 s from each other; the evaluator records statuses every second |

## Commands

```sh
cargo build --release -p v1per-workload
G="target/release/v1per-gate --data /tmp/v1per-gate --disk-reserve-mib 1024"
$G prefill --days 91          # 2026-07-19T15:00Z to 2026-10-18T15:00Z
$G layout
$G queries --warm 50 --cold 10
$G plans --out /tmp/v1per-gate-out/plans
$G live --minutes 10 --expire-after-s 240
$G live --minutes 3 --scale 10
```

The prefill sends one hour of heartbeats per `admit` and one hour of statuses per `record_statuses`, so it runs through the inbox, the stable batches, the ledger and the rollup merges like live data. The live run sends agent batches and evaluator statuses on the real schedule, in real time.

## Results

### Ingestion

| Measure | Result | Target |
| --- | --- | --- |
| Admission per agent batch, 10 min full rate (1,200 batches, 18 heartbeats median, 158 max) | p50 5.5 ms, p95 14.5 ms, p99 26.7 ms, max 61.5 ms, 0 refused | p95 250 ms |
| Admission of evaluator statuses (602 admissions) | p50 8.9 ms, p95 25.3 ms, p99 42.0 ms | none |
| Admission at 10 times the monitors (20,000, 3 min, up to 1,335 heartbeats a batch) | p50 6.9 ms, p95 33.1 ms, p99 80.3 ms, 0 refused | p95 250 ms |
| Transfer lag, acknowledgement to visible in a read | p50 505 ms, p95 972 ms, max 1,048 ms; at 10 times p95 999 ms | none; status changes do not wait for transfer |
| Oldest inbox row while running | p95 959 ms, max 975 ms | none |
| Transfer batch apply in DuckDB (623 batches) | p50 18.6 ms, p95 32.8 ms, p99 47.0 ms | none |
| Of which rollup merges (minute, hour, day) | p50 13.8 ms, p95 25.1 ms, p99 32.2 ms | none |
| Prefill, 91 days through `admit` | 440.6M heartbeats and 366.9M statuses in 942 s (857,000 entries a second); 725 s in DuckDB apply, 276 s of it rollup merges | none |

### Queries

Status page reads are the monitor-level series. `now` is the end of the prefill, 2026-10-18T15:00Z. Milliseconds, queue wait included, p50 / p95. 10 cold samples, 50 warm samples per case.

| Case | Rows | Cold | Warm | Target |
| --- | --- | --- | --- | --- |
| Latest heartbeat of 2,000 monitors (last 5 minutes) | 2,000 | 31.4 / 38.0 | 14.3 / 16.5 | none of its own |
| 90 daily bars, 50 components, Europe/Vienna | 4,500 | 136.6 / 149.8 | 121.9 / 126.6 | page 300 ms |
| 90 daily bars, 50 components, Asia/Kolkata | 4,500 | 137.0 / 142.4 | 123.5 / 128.3 | page 300 ms |
| 30-day latency chart, one monitor, hour buckets | 720 | 13.8 / 15.7 | 9.1 / 10.7 | 1 s |
| Uptime 24 h, one monitor | 1 | 58.8 / 72.7 | 14.1 / 16.0 | none of its own |
| Uptime 24 h, 50 monitors | 50 | 90.4 / 106.9 | 44.6 / 47.0 | none of its own |
| Uptime 7 d, one monitor | 1 | 5.5 / 6.0 | 3.7 / 4.2 | none of its own |
| Uptime 7 d, 50 monitors | 50 | 11.4 / 13.1 | 10.4 / 11.4 | none of its own |
| Uptime 30 d, one monitor | 1 | 2.9 / 3.5 | 2.4 / 2.7 | none of its own |
| Uptime 30 d, 50 monitors | 50 | 4.4 / 5.2 | 4.0 / 4.6 | none of its own |
| Uptime 1 y, one monitor | 1 | 4.0 / 4.6 | 3.5 / 6.6 | none of its own |
| Uptime 1 y, 50 monitors | 50 | 6.7 / 7.8 | 6.0 / 6.8 | none of its own |

Under live load (one read every 2 s during the 10-minute run): bars for Vienna p95 167 ms, max 230 ms; latest of 2,000 p95 26.5 ms. At 10 times the monitors: bars p95 130 ms, latest p95 111 ms.

The briefing has no target for latest status and uptime on their own. The status page target (300 ms server time for the whole page) covers bars, the latest status and the page's uptime; bars plus latest take about 140 ms warm, leaving about 160 ms for SQLite reads and rendering. The page's 90-day uptime comes from the same bars.

### Resources and retention

| Measure | Result |
| --- | --- |
| Process RSS, 10-minute live run | p50 675 MiB, max 1,011 MiB |
| Process RSS, query suite | peak 1,074 MiB |
| Process RSS, prefill (32 MiB transfer batches) | peak 3,005 MiB, see the risk in 0009 |
| DuckDB file, 91 days | 11,499 MiB (11,459 MiB in use, 40 MiB free) |
| Raw heartbeats | 440.6M rows, 5,871 MiB, 14.0 bytes a row, 4.84M rows a day |
| Minute rollups | 573.1M rows, 5,439 MiB, 9.95 bytes a row, 6.30M rows a day |
| Hour rollups | 9.6M rows, 126 MiB, 105,168 rows a day |
| Day rollups | 403,144 rows, 7 MiB, 4,382 rows a day |
| Inbox file after drain | 44 KiB |
| Retention, raw, one day (cutoff now minus 90 days) | 4,981,027 rows in 0.39 s |
| Retention, minute rollups at 90 days, one day | 6,481,634 rows in 0.57 s |
| Admission during retention | 2 agent batches fell inside the 0.96 s it ran, p95 10.3 ms; outside p95 14.5 ms |

Disk per 90 days: raw 5.7 GiB plus minute rollups 5.3 GiB plus hour and day rollups 0.13 GiB, about 11.2 GiB. DuckDB marks deleted rows free and reuses the blocks; the file does not shrink.

Minute rollups for one year at the gate size: 6.30M rows a day times 365 is 2.30 billion rows, about 23 GB (21 GiB) at the measured 9.95 bytes a row. Hour rollups: 38.4M rows a year; day rollups: 1.6M rows a year. These feed Mike's open decision in [0009](../decisions/0009-storage-budgets.md#minute-rollup-retention-open-for-mike).

### Query plans

`EXPLAIN ANALYZE` of the SQL `crates/storage/src/queries.rs` emits, warm, from `v1per-gate plans`.

90 daily bars for 50 components (99 ms total): a sequential scan of `rollup_hour` returns 4,368,076 monitor-level rows of 92 days (`probe_id IS NULL` pushed into the scan, the monitor list only as an optional filter), a mark join keeps 109,200 rows of the 50 monitors, a hash group by sums them per hour. The scan is 0.13 s of thread time. Every row group holds about a day of all monitors, so zone maps on `monitor_id` cannot skip anything. The same read over a temporary copy ordered by (probe, monitor, bucket) took 32 ms.

Uptime 24 h for 50 monitors (43 ms): the scan of `rollup_minute` is pruned to the last day by the bucket bound and returns 2,878,076 monitor-level rows; the mark join keeps 71,950 and the group by returns 50.

Latest of 2,000 monitors (10 ms): the scan of `heartbeat` is pruned to the last 5 minutes and returns 16,848 rows; the window function keeps one per monitor.

## Changes made during the gate

- `crates/storage`: `Health` gained `applied_batches`, `apply_time` and `rollup_time`, cumulative since start, so the bench (and later diagnostics) can split transfer cost per batch. No change was needed for performance.
- `crates/storage`, contract change from [0008](../decisions/0008-monitoring-rules.md): uptime counts pending as down; `UptimeWindow` gained `Week` (168 hour buckets) and `Month` now reads 30 UTC day buckets; imported Kuma rows take their maintenance count from `extras`; day bars drop a zone's offset minutes for the day boundary (+05:30 and +05:45 use +05:00, -03:30 uses -03:00). The earlier code rounded negative non-hour offsets the other way. The query numbers above were measured with these rules.
- `crates/workload` (new, `v1per-workload`): the generator and the `v1per-gate` binary.

## Not verified

- The reference host. Nothing capped the process at 2 cores or 4 GiB; DuckDB's 2 threads and 1 GiB, the 2 Tokio workers and the 2 readers are the only limits. A Linux VM with cgroup limits (as holocron used) was not run.
- Cold OS page cache. All cold samples had the OS cache warm.
- A 24-hour soak, a long backlog drain on a 4 GiB host, and admission over HTTP (the bench calls `Storage::admit` in process).
- Push, group and manual monitors (raw rows with no probe) are not in the mix; their reads use the same monitor-level series.
- Retention ran for under a second, so it says little about whether admission latency moves under a long expiry. Admission never touches DuckDB, so a slow expiry delays visibility, not acknowledgement.
- The failpoint tests (`cargo test -p v1per-storage --features failpoints --test failpoints`) passed 8 of 8 boundaries with 20 seeds each, in a debug build on macOS only; Linux was not run.
