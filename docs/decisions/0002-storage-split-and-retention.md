# 0002: Storage split and retention

Status: Adopted by Mike on 2026-10-18, except the rollup retention item, which is open in the [milestone index](../milestones/README.md#open-decisions). Milestone 02 measures the design and may revise it with evidence.

## Problem

V1Per writes about 4 million check results a day at the target size and must answer status page and uptime queries fast, while keeping configuration changes transactional and simple.

## Constraints

One process, embedded engines, the holocron template, 2 vCPU and 4 GiB, 90 days of raw heartbeats, Kuma uptime numbers kept for imported periods.

## Choice

- SQLite (`control.sqlite`) owns accounts, nodes, grants, monitors, probes, channels, pages, incidents, maintenance, current monitor state, the transitions journal, the notification outbox, the topic log and import state.
- A SQLite result inbox (`synchronous=FULL`) receives probe and push results. An agent batch is acknowledged only after it commits.
- DuckDB (`heartbeats.duckdb`) owns raw heartbeats, minute, hour and day rollups, imported Kuma `stat_*` rows and the applied-batch ledger. Transfer follows holocron's batch ledger protocol.
- Monitor status changes are decided from the admitted results, before transfer, so alerts do not wait for DuckDB.
- Raw heartbeats are kept 90 days.
- Rollups: the agreed default keeps minute, hour and day rollups forever. Open: Mike decides whether minute rollups instead follow raw retention (recommended), since minute rollups forever reach about a billion rows a year at the target size.

## Rejected alternatives

- SQLite only: Kuma's own heartbeat table shows how a large SQLite file slows down uptime queries and retention.
- DuckDB only: DuckDB is a poor fit for many small transactional writes and the durable acknowledgment rule.
- An external time-series database: breaks the one-process requirement.

## Consequences

Backups must capture both engines and ledger positions coherently (11). Status page and uptime queries read rollups, never raw rows, except the latest status.

## Affected contracts

Monitoring contract (rollup fields), milestones 02, 05, 10, 11.

## Amendment: percentiles read raw heartbeats (proposed 2026-11-12, waiting for Mike)

The status page's `response-times.json` reads raw heartbeats for the average and the p50, p95 and p99 of a range, because rollups hold only minimum, maximum, sum and count. Its points still come from rollups, and uptime and status page status stay on rollups as before.

Evidence, 2,000 monitors at 60 seconds over 90 days (311 million raw rows), DuckDB on 2 threads on a laptop, not the reference host: one monitor's read from raw heartbeats as ingested took 13 ms over 24 hours and 1,040 ms over 90 days, because ingestion mixes every monitor into every row group. The hourly retention job now rewrites each closed UTC day once, sorted by monitor, probe and time (`Storage::compact`, about 0.84 s of writer time per day at that size, 76 s for all 90 days on first run). After that the full read takes 4 ms over 24 hours and about 20 ms over 90 days, and the percentiles stay exact. The rewrite is one transaction per day, so a crash leaves the day as it was and the next run repeats it. The `compacted_days` marker table is created on open; it needs no schema version bump and older releases ignore it.

Rejected: approximate percentiles from a sketch column in the rollups, which needs a rollup schema change and a backfill, and gives estimates where exact numbers are cheap.

If refused, the route drops the percentiles and keeps the average from rollups.
