# 0007: Kuma import

Status: Adopted by Mike on 2026-10-18, except the proxy item, which is open in the [milestone index](../milestones/README.md#open-decisions).

## Problem

Mike runs Uptime Kuma 2.5.0 on SQLite. V1Per must take over its monitors, alerts, pages and history in one cutover, with push clients and uptime numbers unchanged.

## Constraints

Kuma 2.5.0 SQLite only. Config and full history. Kuma has no tree. Only push URLs stay compatible. Real data never enters Git.

## Evidence

From read-only inspection of Kuma 2.5.0 (`sources/manifest.json`):

- Schema: template `db/kuma.db`, legacy patches, then 54 knex migrations; the last is `2026-07-22-0000-fix-stat-daily-overflow`. Migration state is in `knex_migrations`.
- `heartbeat` has status 0 to 3, UTC text times and an optional response body. `stat_minutely`, `stat_hourly` and `stat_daily` hold up, down and latency per monitor per bucket.
- Group monitors are monitors with `parent` links. Maintenance has six strategies. Status pages have groups, `monitor_group` links, `status_page_cname` domains and a plain-text password. `incident` is one banner per page.
- `notification.config` is a JSON blob whose `type` picks one of 98 providers.
- Users use bcrypt (cost 10, legacy sha1 rehashed on login) and a plain TOTP secret.
- Push is `ALL /api/push/:pushToken` with `status`, `msg` and `ping`.
- Retention follows the `keepDataPeriodDays` setting, so a source may hold less raw history than expected.

## Choice

- `v1per import kuma --source <kuma.db> --into <node>` opens the source read-only and checks the migration signature and columns. Other versions are refused with guidance.
- `--dry-run` prints counts, support status per monitor type and provider, secrets to seal, heartbeats inside and outside the retention window, and the inventory.
- The real run writes into an empty target node and refuses a second import there. It is resumable by chunks and never changes the source.
- Kuma group monitors become child nodes holding their members. A group monitor is also created on that node, keeping the group's own alerting.
- Monitors of supported types run after import; other types are imported paused with a reason. Push tokens are kept. Each monitor's current state, its probes' state and the last push time are seeded from Kuma's newest heartbeat, so a monitor already down at cutover does not alert again and push deadlines carry over.
- Notifications map to channels on the target node with their monitor links and default flags. Unsupported providers are imported inactive.
- Maintenance, tags, status pages (groups, components, domains, theme, locale) and Kuma incidents map to their V1Per counterparts. A Kuma incident becomes an incident with one update, resolved unless the banner is still shown. Page passwords are hashed; custom CSS and analytics settings are inventoried.
- Kuma users become accounts with grants on the target node; the first admin also becomes instance admin. bcrypt hashes are verified at login and replaced with argon2. TOTP secrets are sealed. Sessions and API keys are not imported; keys are inventoried for reissue.
- Raw heartbeats inside the retention window and all `stat_*` rows go to DuckDB with the pseudo probe `kuma-import`. Imported rollups stay as they are so uptime matches Kuma.
- Docker hosts, remote browsers and other unmapped objects are inventoried with their source representation and a reason.
- Open: monitors that use a Kuma proxy. Recommendation: support HTTP and SOCKS proxies on http types so they keep running; otherwise they import paused.
- CI uses the synthetic fixture from 01. Mike's database is read locally through `V1PER_KUMA_DB`.

## Rejected alternatives

- A re-runnable sync while both run: more work keyed on Kuma IDs for a one-time move.
- Flat import into one node: loses the group structure Kuma users rely on.
- Recomputing rollups from raw heartbeats: numbers drift from Kuma, and raw history is cut by `keepDataPeriodDays`.

## Consequences

The cutover runbook (10) stops Kuma before the final import. Release notes list login, 2FA and API key changes for imported users.

## Affected contracts

Monitoring contract, `docs/contracts/kuma-notifications.md`, `fixtures/kuma-2.5.0`, milestones 01, 10, 11.
