# Milestone 02: workspace and storage gate

## Outcome

The Cargo workspace exists and a measured storage design shows that 2,000 monitors' results can be admitted, transferred to DuckDB, rolled up and read within the release targets on the reference host.

## Dependencies and ownership

Requires 01. Storage and import owns `crates/storage`, its migrations and the benchmark tooling. Integration owns the workspace layout and `apps/server` startup skeleton, copied from holocron.

## Work

1. Create the workspace from holocron's layout: `apps/server` (binary `v1per`), `crates/protocol`, `crates/storage`, pinned toolchain, release profile settings for DuckDB, `dev-clock` feature.
2. Port the control database boundary, the SQLite inbox and the DuckDB batch ledger from holocron, reshaped for heartbeat batches with agent batch IDs.
3. Define the DuckDB heartbeat table and minute, hour and day rollups per the [monitoring contract](../monitoring-contract.md) and [0002](../decisions/0002-storage-split-and-retention.md). Include a place for imported Kuma rollups.
4. Port secret sealing from holocron per [0006](../decisions/0006-secrets-at-rest.md).
5. Build a workload generator for the release gate mix (400 monitors at 20 s, 1,600 at 60 s, 10 probes, flapping and outage patterns) and a query suite: latest status per monitor, 90 daily bars for 50 components in a page timezone, 30-day latency chart, 24 h, 30 d and 1 y uptime.
6. Measure admission latency, transfer lag, rollup cost, query latency, disk use for 90 days and retention deletion on the reference budget. Record the engine design and budgets in a decision.

## Acceptance criteria

- Admission acknowledges only after a durable SQLite commit; a killed process at every transfer step loses and duplicates nothing.
- The query suite meets the targets at 90 days of generated data, or a decision records the failing workload and alternatives.
- Retention removes raw heartbeats older than 90 days without blocking admission.
- Mike has confirmed or adjusted the release targets and the rollup retention.

## Evidence and handoff

Report machine, versions, workload manifest, measurements and query plans in `docs/evidence/m02-storage-gate.md`. Hand the storage interface to 05 and 10.
