# Milestone 11 operations and recovery evidence

Measured on 2026-10-18 by the storage and operations owner. Design in [0010](../decisions/0010-operations-and-recovery.md), procedures in the [runbook](../operations/runbook.md).

Verdict: a backup taken while the server serves restores into a fresh directory with every result it accepted exactly once. The restored server resumes checks, sends the pending notification with the restored key and serves its status page. Restore refuses a backup without its key, with another key, into a used directory and from a damaged file, and leaves the directory empty. Disk-full and interrupted retention and backup recover on the next run. The milestone's packaging work (Dockerfile, probe image) and the admin screen are not part of this task.

## Machine and versions

| Item | Value |
| --- | --- |
| Machine | Apple M1 Pro laptop, 10 cores, 32 GiB RAM, macOS 27.0.1. Not the reference host |
| Build | debug profile, no release build: the disk had about 29 GiB free |
| Toolchain | rustc 1.99.1 |
| Engines | DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite 3.51.3 (SQLx 0.9.0) |
| Source | commit `c515916` plus the uncommitted work of milestones 02 to 11 |

## Clean-machine restore rehearsal

A scratch script drove the `v1per` binary over HTTP. The source and the "new machine" were two fresh temp trees that shared nothing but the archive and a copy of `secrets.key`; the new machine never read the source's data directory.

1. `v1per init`, `v1per serve` on port 18431. Created a push monitor (interval 60 s), an HTTP monitor of the server's own `/health` (20 s, built-in probe) and a public page showing the push monitor.
2. Pushed `rehearsal-1` to `rehearsal-30`, then kept pushing one beat every 0.2 s while `v1per backup --data-dir A --out B/backups/v1per.tar` ran against the serving instance.
3. `v1per diagnostics` on the source, stopped the source with SIGTERM.
4. On the new machine, four refused restores, then `v1per restore --secrets-key-file`, `v1per serve` on port 18432, checks.

| Step | Result |
| --- | --- |
| Backup while serving | 2.0 MiB archive, transfer paused 21 ms, ledger at batch 2 with 34 records applied and 3 pending, key fingerprint printed, key not included |
| Pushes acknowledged by the source | 37 |
| Restore without a key | exit 3 `secrets_key_required`, names the fingerprint |
| Restore with a random key | exit 3 `secrets_key_mismatch`, names both fingerprints |
| Restore into a directory with a file | exit 3 `directory_not_empty` |
| Restore of the archive with one byte flipped in the middle | exit 3 `backup_corrupt` (`heartbeats.duckdb does not match its checksum`); target directory left empty |
| Restore with the key file | `ready`, 3 inbox records moved into DuckDB, `secrets.key` written |
| Push beats in the restored instance | 36, all distinct, `rehearsal-1` to `rehearsal-36` without a gap. Push 37 arrived after the copy and is not in the backup |
| Push after restore | accepted |
| Status page `/s/status/snapshot.json` | 200 |
| HTTP monitor | 2 new beats 25 s after start: checks resumed |
| `v1per retention --dry-run` and `v1per diagnostics` against the serving restored instance | answered through the job queue |

An earlier run of the same script, cut short by a shell error after the refusals, showed the same shape: 41 pushes acknowledged, a backup with 39 applied and 2 pending records and a 35 ms pause.

## Tests

`cargo test -p v1per-server` and `cargo test -p v1per-storage --features failpoints`, all passing.

| Behavior | Test |
| --- | --- |
| Backup while results arrive, restore on a clean directory: every batch the backup holds once, resent batches recognized, the pending delivery sent after restore, the page listed | `apps/server/tests/operations.rs` `a_backup_while_running_restores_without_duplicates` |
| Restore refuses without key, with another key, a used directory, an initialized one, a damaged archive; a backup with `--include-key` needs no key; an existing `--out` is refused | `restore_refuses_what_it_cannot_restore_whole` |
| Retention dry run counts what the run removes (heartbeats, sessions, deliveries), removes nothing, live sessions stay | `the_retention_dry_run_counts_what_the_run_removes` |
| Commands queued for a server that holds the lock; diagnostics JSON shape and text; `instance.operations` matches it and refuses non-admins | `commands_run_on_the_server_and_diagnostics_report_its_state` |
| Disk full during a backup: `disk_full`, no partial files, transfer resumes, the next backup verifies | `apps/server/tests/operations_disk_full.rs` |
| Disk full in the second retention step: the first day stays deleted, the next run removes the rest | same file, and `crates/storage/tests/disk_full.rs` |
| A snapshot with results waiting in the inbox restores every heartbeat once; dry-run counts equal deletions | `crates/storage/tests/backup.rs` |
| Process kill at every admission and transfer boundary, 20 seeds each | `crates/storage/tests/failpoints.rs` (unchanged, passing) |
| Pages list access modes; `page.*` events carry `page`; a newly readable beat shows in the next snapshot | `apps/server/tests/pages.rs` |
| `serve` start rules for current, older (with and without `--migrate`), newer and half-upgraded data | `maintenance::versions` unit test |

## Not verified

- A real full disk. Disk-full cases use injected `StorageFull` errors at the DuckDB copy and between retention steps, not a filled volume.
- A process killed during retention or a backup. Retention deletes one day per DuckDB transaction, so a kill loses at most the step in hand; this is DuckDB's guarantee, not a test of ours.
- A restore on a second physical machine, and the release build. The rehearsal used two temp trees on one laptop and the debug binary.
- Backup and restore at the release-gate size (an 11.5 GB DuckDB file): pause length, the twice-DuckDB space need and restore time are unmeasured.
- `v1per upgrade` on real older data: there is only one schema release so far. The version rules are unit tested; the migration path runs the same `Db::open` and `Storage::open` as `serve`.
- Docker: `docker exec v1per v1per backup` and `V1PER_MIGRATE` in the compose files.
- Key rotation: not built.
