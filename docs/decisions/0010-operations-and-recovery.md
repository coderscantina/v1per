# 0010: Operations and recovery

Status: Proposed (milestone 11, storage and operations owner). Ported from holocron's 0014 and cut down to what V1Per has: one control database, the inbox and DuckDB, no erasure or compaction. Mike decides the open items at the end. Evidence: [m11 operations and recovery](../evidence/m11-operations-and-recovery.md).

## Problem

[0002](0002-storage-split-and-retention.md) requires backups that capture both engines and the ledger positions coherently, and [0006](0006-secrets-at-rest.md) requires a restore that refuses to run without the matching secrets key. Retention from [0009](0009-storage-budgets.md) has to run on a schedule, and the instance admin needs to see the server's operational state.

## Constraints

- One process and one data directory. DuckDB allows one process per file, so an operator command cannot open it beside a running server.
- Probe results are acknowledged only after a SQLite commit, and transfer applies stable batches exactly once through the ledger. Checks and push endpoints must keep working during a backup.
- Secrets never leave the server in clear text unless the operator asks for it.

## Choice

### Jobs and the data directory lock

- `v1per backup`, `retention` and `diagnostics` are jobs in `maintenance_jobs` (control migration `0010`). If `serve` holds `v1per.lock`, the command queues the job and polls its row; the server's runner picks it up within a second. Without a server the command takes the lock, opens the engines as `serve` does and runs the queue itself. One code path either way, and no socket or admin HTTP route.
- The server queues retention hourly, the first time 10 minutes after start (see agent batch IDs below). A job interrupted by a stop is marked failed with `interrupted` at the next start.
- `restore` and `upgrade` work on a directory no process holds.

### Backup

- One tar file (`--out <file>`, mode 0600): `manifest.json` first, then `secrets.key` only with `--include-key`, then `control.sqlite`, `inbox.sqlite` and `heartbeats.duckdb`.
- The manifest holds the format, the release, the schema version of each file, the ledger positions (last applied batch and sequence, records applied, records still pending), the pause, the key reference (fingerprint, whether the key lives in the data directory, whether it is included) and the size and BLAKE3 hash of every file.
- Coordinated pause: the DuckDB writer is held while DuckDB is checkpointed and its file copied and the inbox copied with `VACUUM INTO`. Transfer and retention wait; admission and reads go on and the inbox absorbs new results (1 GiB budget). The control database is copied right after, through its own read-only connection, so it may hold a few seconds more of state than the heartbeats.
- The pair is consistent because nothing moves batches while the writer waits: a batch applied but not yet retired is in both copies, and transfer skips it after a restore because the ledger already holds it.
- Files are staged next to the archive, hashed, archived (each staged file deleted once written), the archive read back and checked, and only then renamed from `.<name>.partial`. A failure removes both.
- The key fingerprint is the first 16 bytes of SHA-256 over a fixed label and the key. `--include-key` prints a warning to stderr and marks the manifest.

### Restore

`v1per restore --from <file> --data-dir <dir>` refuses a used directory, reads the manifest, refuses a newer schema, then resolves the key: the one given with `--secrets-key-file` or `V1PER_SECRETS_KEY`, else the one in the archive. Without one it exits 3 `secrets_key_required`, with another one `secrets_key_mismatch`, both naming the fingerprint the backup needs. Only then it locks the directory, writes `v1per.restore-in-progress`, extracts with hashing and compares to the manifest (`backup_corrupt`; a refused restore leaves the directory empty). `secrets.key` is written when the instance kept it in its data directory. At the release's schema it opens everything once, requires an instance admin, lets transfer apply the inbox, and requires `ledger records = manifest applied + manifest pending` with nothing left pending. Then it writes `v1per.initialized`.

### Schema versions and upgrade

- Versions are the latest applied migration of `control.sqlite` and `inbox.sqlite` and `schema_info.heartbeat_schema` of DuckDB. A missing file counts as current.
- `serve` checks them before opening anything: newer is `data_newer_than_binary`, an upgrade marker `upgrade_interrupted`, older `upgrade_required` unless `--migrate` (`V1PER_MIGRATE`) is set. All exit 3.
- `v1per upgrade --backup-to <file>` takes a verified backup of the stopped directory, writes `v1per.upgrade-in-progress` naming it, migrates control and then the inbox, checks the versions and removes the marker. A rerun resumes with the same backup.

### Retention

One job does everything that expires, each step idempotent, so a failed or interrupted run is finished by the next one:

| What | When |
| --- | --- |
| Raw heartbeats | after 90 days, one UTC day per DuckDB transaction |
| Minute rollups | kept until Mike decides ([0002](0002-storage-split-and-retention.md)); `Retention::minute_rollups` takes the choice |
| Agent batch IDs | after 24 hours |
| Finished deliveries, finished jobs | after 30 days |
| Sessions, login challenges, page sessions, sign-in links | once expired (sessions also when idle 7 days) |
| Command answers | after their 24 hour replay window |

This replaces the session sweep and the outbox prune that ran in their own loops. `v1per retention --dry-run` counts the same rows and deletes nothing.

Agent batch IDs: an agent resends a batch until it is acknowledged, so its ID only has to outlive a lost acknowledgement. A day holds about 860,000 IDs at 10 probes; a week would hold six million. The first run waits 10 minutes after start so agents cut off by an outage resend first.

### Diagnostics

`instance.operations` (instance admins) and `v1per diagnostics [--json]` return the same document: versions, engine file sizes, data directory size, disk free and reserve, inbox depth and age, the watermark and transfer failures, every probe with its health and lag counters, outbox backlog and failures of the last day, retention policy and last run, the 20 newest jobs and the imports. It is built from counts, sizes, states and names only.

### Page snapshots

A cached page snapshot is valid while the page topic's head and the heartbeat watermark stay where they were when it was built, for at most a minute. Bars no longer lag a readable beat by up to 60 seconds; under steady load a page is rebuilt about once a second per audience and locale.

## Rejected alternatives

- A backup directory as in holocron: the brief asks for one file an operator can move. Tar needs no new format and `tar -tf` lists it.
- Manifest last in the archive: a restore would extract everything before it could say the key is missing.
- Copying the live files without the pause: the inbox and the ledger would disagree, and a restore could apply a batch twice or not at all.
- An admin HTTP route or socket for operator commands: a second protocol where the control database already serves.
- A shorter snapshot cache time instead of the watermark: any fixed time either keeps stale bars or rebuilds pages that did not change.

## Unresolved risks

- A backup needs free space on the backup volume of about the DuckDB file twice (staged copy and archive) while it runs.
- The control copy can be a few seconds newer than the heartbeat copy. A restored monitor state may reflect beats the restored history lacks; the next checks correct it.
- Agent batches acknowledged after the backup are lost on restore (the agent deleted them). Nothing is duplicated.
- The rebuild rate of page snapshots under load is unmeasured at 2,000 monitors.
- Key rotation is not built.

## Open for Mike

- Minute rollup retention (from 0002 and 0009, unchanged).
- `serve` now refuses older data without `--migrate`. Docker deployments that want unattended upgrades set `V1PER_MIGRATE=1`; the alternative is migrating at start as before, without a backup.

## Affected contracts

Control migration `0010_maintenance_jobs.sql`; protocol `instance.operations`, `PageSummary.access_modes`, `LiveEvent.page`; [status page API](../contracts/status-page-api.md) cache wording (owner: integration); milestones 11 and 12.
