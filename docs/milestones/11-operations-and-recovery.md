# Milestone 11: operations and recovery

## Outcome

An operator installs v1per with Docker or a binary, backs it up, restores it on a clean machine and sees its health, all with documented commands.

## Dependencies and ownership

Requires 02 and 04; import state integration after 10. Storage and import owns `maintenance` and packaging. Integration owns diagnostics and health.

## Work

1. Port holocron's `backup`, `restore`, `retention` and `diagnostics` subcommands and the coordinated maintenance pause, covering SQLite, DuckDB, inbox, ledger, schema versions and the secrets key.
2. Port the Dockerfile, package script and Coolify compose file; add a probe-only image or entrypoint.
3. Expose operational state in the admin UI for the instance admin: inbox depth and age, DuckDB watermark, probe lag and health, outbox backlog and failures, disk use and retention runs.
4. Write `docs/operations/runbook.md`: install, configure, reverse proxy and custom domains, probe deployment, backup, restore, upgrade, key rotation.

## Acceptance criteria

- A backup restored on a clean machine resumes checks, pending notifications and live pages without duplicate heartbeats.
- Restore refuses a backup without its secrets key and says why.
- Disk-full and interrupted-retention cases recover.

## Evidence and handoff

Clean-machine restore rehearsal in `docs/evidence/m11-operations-and-recovery.md`. Hand to 12.
