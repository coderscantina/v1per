# Milestone 10 Kuma import and cutover evidence

Measured on 2026-10-18 by the storage and import owner.

Verdict: the synthetic fixture and Mike's real Kuma 2.5.0 database both import reconciled. Every monitor's 24 hour, 30 day and 1 year uptime equals Kuma's, 49 of 49 on the fixture and 69 of 69 on the real database. An import killed in each step resumes without duplicates, and the source file hashes the same before and after. The cutover runbook is drafted but not rehearsed against a running Kuma.

## Machine and versions

| Item | Value |
| --- | --- |
| Machine | Apple M1 Pro laptop, 10 cores, 32 GiB RAM, macOS 27.0.1. Not the reference host |
| Build | debug profile (`cargo build`), no release build: the disk had about 48 GiB free |
| Toolchain | rustc 1.99.1 |
| Engines | DuckDB 1.5.6 (duckdb-rs 1.10506.0, bundled), SQLite through SQLx 0.9.0 (libsqlite3-sys 0.37.0) for the data directory and rusqlite 0.39.0 for the source |
| Source | commit `c515916` plus the uncommitted work of milestones 02 to 10 |

## Commands

```sh
cargo run -p v1per-kuma-fixture -- /tmp/v1per-m10/kuma.db
v1per init --data-dir /tmp/v1per-m10/data --admin-email admin@example.com
v1per import kuma --data-dir /tmp/v1per-m10/data --source /tmp/v1per-m10/kuma.db --into Root --dry-run --raw-days 60
v1per import kuma --data-dir /tmp/v1per-m10/data --source /tmp/v1per-m10/kuma.db --into Root --raw-days 60
v1per serve --data-dir /tmp/v1per-m10/data --bind 127.0.0.1:18089
cargo test -p v1per-server --test imports
```

The fixture's history ends on 2026-10-01, so `--raw-days 60` keeps part of it inside the raw window when run on 2026-10-18. The tests compute the window the same way, 30 days into the fixture, whatever day they run.

For the real database, with `V1PER_KUMA_DB` pointing at the local `kuma/kuma.db`:

```sh
v1per init --data-dir /tmp/v1per-m10-real --admin-email admin@example.com
v1per import kuma --data-dir /tmp/v1per-m10-real --into Root --dry-run
v1per import kuma --data-dir /tmp/v1per-m10-real --into Root
sqlite3 /tmp/v1per-m10-real/control.sqlite "UPDATE channels SET active = 0"
v1per serve --data-dir /tmp/v1per-m10-real --bind 127.0.0.1:18090
```

The channels of the scratch copy were switched off before `serve`, so the rehearsal could not alert real channels. The temp data directory was deleted afterwards.

## Fixture results

Synthetic Kuma 2.5.0 fixture (`fixtures/kuma-2.5.0`), 27 MB, imported into the root node of a fresh data directory.

| Check | Result |
| --- | --- |
| Configuration and inventory tables | 22 of 22 reconciled: every source row mapped or inventoried, and v1per's counterpart table holds the mapped count |
| Nodes from group monitors | 6 of 6 |
| `stat_minutely`, `stat_hourly`, `stat_daily` | 10,423, 2,467 and 1,246 rows, all imported |
| Heartbeats | 193,943 in the source: 30,663 inside the 60-day window imported, 163,280 older left to the rollups |
| Uptime 24 h, 30 d, 1 y | 49 of 49 monitors equal Kuma's up and down counts, including monitors with outages in every window |
| Inventory | 56 entries: 28 monitors imported paused, custom CSS, analytics, settings, API keys, proxies, docker hosts, a remote browser, domain expiry, TLS info |
| Logins | 1, `fixture-admin@kuma.invalid`, password and 2FA kept, instance admin |
| Source | SHA-256 the same before and after |
| Time | 5.2 s for the import, 0.35 s for the dry run, debug build |
| After import | `serve` starts; `GET /api/push/<fixture token>` answers `{"ok":true}` |

`cargo test -p v1per-server --test imports`: 4 tests, about 19 s, passed three runs in a row.

- A full import is reconciled, its manifest file equals the returned manifest, the source hash is unchanged, a second import into the node is refused with `already_imported`, and a dry run says so.
- The push URL of an imported push monitor answers with the old token. The Kuma user signs in with the fixture password, gets the TOTP challenge, and the fixture authenticator secret passes it; the bcrypt hash is argon2 after that first sign-in. The `client-acme` page asks for its password at `/s/client-acme` and opens with the Kuma one. `imports.get` and `imports.inventory` answer the instance admin, the inventory names source columns without values, and a member gets `forbidden`.
- `v1per import kuma` processes are killed inside the control transaction, after three `stats` chunks and after three `heartbeats` chunks, with 200-row chunks. After each kill the control step is all or nothing and `serve` refuses with `import_in_progress`. The last process finishes reconciled, so no chunk was counted twice.
- An import beside a process holding the data directory is refused with `server_running`, a node that does not exist with `node_not_found`, and a node with children with `import_conflict`.

## Real database

Mike's Kuma 2.5.0 database, read in place and never copied. Aggregates only.

| Check | Result |
| --- | --- |
| File | 28 MB, no WAL, opened immutable |
| Dry run | every table accounted, target node ready |
| Monitors | 69 of 6 types: 66 run, 3 paused in Kuma, none unsupported |
| Nodes from group monitors | 18 of 18 |
| Channels | 1 of a supported provider, active |
| Secrets | 1 channel config sealed, 1 TOTP secret sealed, no monitor credentials, no page passwords |
| Inventory | 74 entries: 47 TLS info, 14 settings, 7 domain expiry, 4 custom CSS, 2 orphaned links |
| Configuration tables | 22 of 22 reconciled |
| `stat_minutely`, `stat_hourly`, `stat_daily` | 92,037, 49,034 and 11,652 rows, all imported |
| Heartbeats | 138,884: 134,710 inside the 90-day window imported, 4,174 older |
| Uptime 24 h, 30 d, 1 y | 69 of 69 monitors equal Kuma's counts, a 100 % match rate |
| Logins | 1, password and 2FA kept |
| Source | SHA-256 the same before and after the dry run and the import |
| Time and memory | 9.6 s wall clock, 215 MiB peak resident memory, debug build |
| After import | `serve` started without errors; see the run below |

`v1per serve` on the imported copy, 154 seconds: 61 of the 66 active monitors recorded a beat, 58 up and 3 pending, through the built-in probe for the 48 http, keyword, json-query and ping monitors and the server for the 18 groups. The 5 without a beat are the 5 active monitors whose interval is longer than the run. No errors in the log, no outbox rows (channels off).

## Not verified

- The cutover runbook against a running Kuma: stopping it, copying `kuma.db` with `-wal` and `-shm` from a live install, DNS and push client switching, and rollback. Only the import, dry run and first start were run.
- Sign-in, TOTP and the page password on the real database: the real credentials are not known to the test and were not used.
- Alerts from the real import: its channel was switched off for the rehearsal, so no notification was sent.
- A source with a non-empty WAL (Kuma not stopped cleanly). The fixture and the real file have none; `v1per-kuma-import` covers that mode with its own tests.
- Release build timing and the reference host. Times above are debug builds on a laptop.
- A Kuma database large enough to need the inbox budget's back-pressure during the heartbeat step.
