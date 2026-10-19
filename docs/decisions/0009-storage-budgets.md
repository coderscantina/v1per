# 0009: Storage budgets

Status: Proposed (milestone 02). The budgets below are measured on a developer laptop, not the reference host, and change no release target. The minute rollup retention is Mike's open decision from [0002](0002-storage-split-and-retention.md); this record gives the numbers and a recommendation. Evidence: [m02 storage gate](../evidence/m02-storage-gate.md).

## Problem

[0002](0002-storage-split-and-retention.md) fixed the engine split. Milestone 02 has to show that the release-gate workload (2,000 monitors, 10 probes, 90 days of raw heartbeats) fits the 2 vCPU, 4 GiB reference host, and record the engine budgets the server starts with.

## Constraints

- Durable SQLite commit before an agent batch is acknowledged; exactly-once transfer through the batch ledger.
- One DuckDB instance shares its threads and memory between the writer and every reader. DuckDB has no per-query memory limit, so budgets are per installation.
- The release targets in the [technical briefing](../technical-briefing.md#release-gate) stay as they are.

## Evidence

Release build on an Apple M1 Pro laptop (10 cores, 32 GiB, macOS 27.0.1, internal SSD), with DuckDB and Tokio limited to the reference budget below. Nothing limits the process to 2 cores or 4 GiB, and the OS page cache was never cleared.

| Check | Result | Target |
| --- | --- | --- |
| Admission per agent batch, full rate, 10 minutes | p50 5.5 ms, p95 14.5 ms, p99 26.7 ms, 0 refused | p95 250 ms |
| Acknowledgement to visible in a read | p95 972 ms, max 1,048 ms | none; status changes do not wait for it |
| Transfer batch apply, of which rollup merges | p95 32.8 ms, of which 25.1 ms rollups | none |
| 90 daily bars, 50 components, Vienna and Kolkata | warm p95 127 to 128 ms, cold p95 142 to 150 ms; under load p95 167 ms | 300 ms for the whole page |
| 30-day latency chart, one monitor | warm p95 10.7 ms, cold p95 15.7 ms | 1 s |
| Uptime 24 h, 7 d, 30 d, 1 y, one and 50 monitors | slowest: 24 h for 50 monitors, warm p95 47 ms, cold p95 107 ms | none of its own |
| Retention of one day | 4.98M raw rows in 0.39 s, 6.48M minute rollup rows in 0.57 s | must not block admission |
| Process RSS, live run | max 1,011 MiB | 4 GiB host |
| Disk, 91 days | 11.5 GB DuckDB file: raw 5.7 GiB, minute rollups 5.3 GiB, hour and day 0.13 GiB | none |

## Choice

| Budget | Value | Basis |
| --- | --- | --- |
| DuckDB threads, writer and readers together | 2 | two vCPUs |
| DuckDB memory limit | 1 GiB | holocron's 0008: 1.5 GiB was OOM-killed on a 4 GiB host; the live run peaked at 1,011 MiB RSS with 1 GiB |
| DuckDB build | release without debug info, so `NDEBUG` is set | root `Cargo.toml`, as in holocron |
| Tokio worker threads | 2 | two vCPUs; the gate ran with 2 |
| Readers / queue / timeout | 2 / 8 / 30 s | page reads stay under 250 ms with transfer running |
| Transfer batch | 20,000 inbox rows or 32 MiB encoded | a live batch holds about 100 entries; the limits only matter when a backlog drains |
| Transfer delay after a commit wakeup | 1 s | visibility p95 under 1 s; status changes are decided before transfer, so only charts and uptime wait |
| Retention step | one UTC day per delete, writer released in between | one day took 0.39 s (raw) and 0.57 s (minute rollups); admission never touches DuckDB |
| Inbox backlog / admission queue | 1 GiB / 64 MiB encoded | the live inbox never held more than 1 s of results |
| Disk reserve | 4 GiB free | as holocron; the laptop run used 1 GiB because the volume had 12 GiB free |

These are the values `Budgets::default()` already carries; the gate did not need to change any of them.

### Minute rollup retention (open, for Mike)

Measured: the minute rollup holds one row per series and minute, and a series is a (monitor, probe) pair or a monitor-level series. With 2,000 monitors on 2,382 probe series plus 2,000 monitor series that is 6.30M rows a day, 2.30 billion rows a year, at 10 bytes a row: about 23 GB (21 GiB) a year, growing without bound if kept forever. 0002 estimated about a billion rows a year; the measured figure is 2.3 times that, because every monitor-level and every per-probe series gets a row each minute.

Nothing reads minute rollups beyond 24 hours: the 24 h uptime is the only minute-resolution read, and 0008 builds bars for non-hour zones from hour rollups. Recommendation: keep minute rollups as long as raw heartbeats, 90 days (about 5.6 GB), and hour and day rollups forever (38M and 1.6M rows a year). `Retention::minute_rollups` takes the choice; expiring one day of them took 0.57 s.

## Rejected alternatives

- A larger DuckDB memory limit: no query came near 1 GiB, and holocron lost a 4 GiB host to the OOM killer at 1.5 GiB.
- A shorter transfer delay: visibility is already under 1 s and every batch costs a ledger row and three rollup merges.
- Bulk loading straight into DuckDB for the gate: the prefill went through `admit`, `record_statuses`, the inbox and transfer (857,000 entries a second), so the measured layout is the one live ingestion produces.

## Unresolved risks

- **Bars scale with the installation, not the page.** The 90-day bar read scans every monitor-level hour row of 92 days (4.37M rows at 2,000 monitors) because live ingestion interleaves all monitors in every row group, so zone maps cannot skip other monitors. At 2,000 monitors it takes 99 ms of SQL; it grows about in proportion to the monitor count and would reach the 300 ms page budget alone near 5,000 monitors. A copy of the hour rollups clustered by (series, monitor, bucket) ran the same read in 32 ms. If more headroom is needed, a compaction job that rewrites closed hour-rollup ranges in that order is the measured option; it is not built.
- **Memory while a backlog drains.** The prefill applied 32 MiB transfer batches and peaked at 3.0 GiB RSS. Live batches never come near that size, but an inbox that builds up during a DuckDB outage drains in full-size batches. A backlog drain on a 4 GiB host is unmeasured; if it is too close, lower `batch_bytes` to 8 MiB.
- **Raw rows exceed the briefing estimate.** The briefing says about 360 million raw rows for 90 days; with 10 % of monitors on three probes the mix produces 4.84M a day, 440M for 91 days. The measurements above use the larger number.
- The reference host, a 2-core and 4 GiB limit, cold OS cache and a 24-hour soak are unverified.

## Consequences

- The server starts with `Budgets::default()` and two Tokio workers.
- Milestone 05 reads uptime through `UptimeWindow::{Day, Week, Month, Year}` (0008 windows) and status pages read `day_bars`.
- Milestone 11 schedules `Storage::expire` daily with the retention Mike picks.
- Milestone 12 reruns `v1per-gate` on the reference host with agents, notifications and pages running.

## Affected contracts

[Technical briefing](../technical-briefing.md#release-gate) release gate, [0002](0002-storage-split-and-retention.md) rollup retention, milestones 05, 09, 11 and 12.
