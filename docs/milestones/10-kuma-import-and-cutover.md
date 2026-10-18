# Milestone 10: Kuma import and cutover

## Outcome

An operator imports a Kuma 2.5.0 SQLite database into a node, sees what was mapped and what was not, switches DNS and push clients over, and finds monitors running, alerts arriving and uptime numbers matching Kuma.

## Dependencies and ownership

Requires 05, 07, 08 and 09; inspection and the fixture start in 01. Storage and import owns `imports`. Frontend owns the import report screen.

## Work

1. Implement inspection per [0007](../decisions/0007-kuma-import.md): open read-only, check the `knex_migrations` signature and columns, refuse other versions with guidance.
2. Implement `--dry-run`: counts per table, monitors by type with support status, notifications by provider, secrets to seal, heartbeats inside and outside retention, inventory preview.
3. Map config: users, monitors, group nesting into child nodes, tags, notifications and their links, maintenance, status pages with groups, domains and passwords, incidents, settings that have a v1per equivalent. Seal secrets, hash page passwords, keep push tokens.
4. Import raw heartbeats inside the retention window and all `stat_*` rows through the storage interface in resumable chunks.
5. Inventory everything not mapped with a reason: unsupported types (imported paused), unsupported providers (imported inactive), docker hosts, remote browsers, API keys, custom CSS, analytics settings.
6. Reconcile counts and uptime numbers per monitor against the source and write a manifest. Refuse a second import into the same node.
7. Write the cutover runbook: stop Kuma, final import, verify, switch DNS and push URLs, keep Kuma for rollback.

## Acceptance criteria

- The synthetic fixture imports in CI with a reconciled manifest, and an interrupted import resumes without duplicates.
- Mike's real database imports locally through `V1PER_KUMA_DB`; 24 h, 30 d and 1 y uptime match Kuma for every monitor.
- The imported Kuma user signs in with the old password and authenticator.
- The source file is unchanged after import.

## Evidence and handoff

Manifests, inventory, the real-data comparison (counts only, no secrets) and the runbook rehearsal in `docs/evidence/m10-kuma-import-and-cutover.md`. Hand import state to 11.
