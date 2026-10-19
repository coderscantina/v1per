# 0008: Monitoring rules

Status: Adopted (milestone 01, team decision) unless an item says otherwise. Items 12 and 13 are proposed, awaiting Mike.

## Problem

The [monitoring contract](../monitoring-contract.md) left status, retry, group, push, maintenance and uptime rules to be checked against Uptime Kuma 2.5.0. v1per imports Kuma history and keeps computing uptime from cutover on, so a rule that differs from Kuma makes the numbers jump on the day of the move.

## Constraints

- Imported `stat_*` rows are kept as they are (contract, confirmed). New rollups must merge with them without a visible break.
- Kuma runs one check location per monitor. v1per adds probes and a quorum, which Kuma has no rule for.
- Where the contract's proposal and Kuma differ, follow Kuma unless the difference is a product choice for Mike.

## Evidence

Read from Uptime Kuma 2.5.0 at commit `d9a60dfc` (`sources/manifest.json`). Paths are relative to the Kuma source root.

- Status values: `DOWN = 0`, `UP = 1`, `PENDING = 2`, `MAINTENANCE = 3` (`src/util.ts:32-35`).
- Uptime counting: `update()` adds a maintenance beat to a separate `maintenance` counter and to neither `up` nor `down` (`server/uptime-calculator.js:231-234`). Every other status goes through `flatStatus()`, which maps pending to down (`:545-555`, `:278-282`). Uptime is `up / (up + down)`, and 0 when both are 0 (`:681-685`).
- Latency: only up beats with a numeric ping change the running average, minimum and maximum (`:235-277`). Windows merge buckets weighted by `up` (`:620`, `:673-679`).
- Buckets: minute and hour keys are unix seconds at the start of the minute or hour; day keys are the start of the UTC day (`:448-517`). Extra counters such as `maintenance` are stored as JSON in `extras` (`:308-314`, `:328-334`, `:347-353`).
- Windows: 24 hours sums 1440 minute buckets, 7 days 168 hour buckets, 30 days 30 UTC day buckets and 1 year 365, each including the current bucket (`:563-597`, `:806-829`). An empty window falls back to the last bucket the calculator saw (`:641-671`).
- Retention: minute rows are deleted after 24 hours and hour rows after 30 days on every beat (`:63-64`, `:357-371`). Heartbeats and day rows are deleted after `keepDataPeriodDays`, default 365, and a value below 1 disables it (`server/jobs/clear-old-data.js:7`, `:33-49`).
- Retries: a failed check is pending while `retries < maxretries` and down after that; the counter keeps rising while down and resets on success (`server/model/monitor.js:946-958`, `:913`). A pending beat schedules the next check after `retry_interval` when it is above 0, otherwise after `interval` (`:1035-1038`). For `json-query` with `retry_only_on_status_code_failure`, a failed JSON query is down at once (`:929-945`).
- Upside down: the default status is flipped before the check and the check result after it; a flipped success throws "Flip UP to DOWN" and goes through retries, a flipped failure is up and resets retries (`:459-461`, `:905-911`, `:925-928`). `flipStatus` leaves pending and maintenance alone (`src/util.ts:169-179`).
- Important beats: the first beat and the transitions up/down, pending to down, and any change into or out of maintenance are marked `important` (`server/model/monitor.js:1385-1411`). Notifications go out for the first beat, up to down, pending to down, down to up and maintenance to down only (`:1420-1443`), and a first beat notifies only when it is down (`:1452-1453`). A down beat that is not important increments `downCount`; when it reaches `resend_interval` (a beat count) the down notification is sent again and the count resets (`:989-1001`).
- Push: the server's loop first runs one interval after start (`:1106-1113`) and reloads the latest beat each time (`:444-449`). If that beat is not up (not down for upside-down monitors) or is older than `interval` plus a 1 s buffer, the loop records "No heartbeat in the time window" through the normal retry path; otherwise it sleeps until the deadline and records nothing (`:746-777`). A push request (`ALL /api/push/:pushToken`) records `up` unless `status` is something else, which means down; `msg` defaults to "OK" and `ping` is optional (`server/routers/api-router.js:47-89`). Its retry handling in `determineStatus` gives the same pending and down sequence (`:576-619`).
- Groups: a group with no children is pending "Group empty"; paused children are skipped; a child without beats counts as pending; a down child makes the group fail, a pending child makes it pending; up and maintenance children count as fine (`server/monitor-types/group.js:12-74`). The failure is thrown, so the group's own retries and upside down apply (`:72-73`). Only direct children are read, using each child's latest beat (`server/model/monitor.js:1587-1589`, `:1919-1927`). Pausing a group does not stop its children's checks; the UI shows them as paused (`server/server.js:1934-1945`, `server/model/monitor.js:1301-1305`).
- Maintenance: a monitor is under maintenance when one of its own maintenance windows is active or any ancestor group is under maintenance (`server/model/monitor.js:1596-1618`). The check is then skipped and the beat is `MAINTENANCE` with message "Monitor under maintenance" (`:470-472`); a push during maintenance is stored as maintenance (`server/routers/api-router.js:84-86`). A window's state is `inactive`, `scheduled`, `under-maintenance`, `ended` or `unknown`: manual is active until switched off, single runs between its start and end dates, the recurring strategies and cron start on their cron in the window's timezone and last `duration` seconds, all bounded by the date range (`server/model/maintenance.js:421-449`, `:231-340`, `:455-503`).
- Manual monitors: no status set means pending "Manual monitoring - No status set"; otherwise the set status, recorded every interval (`server/monitor-types/manual.js:16-32`).
- Status pages: Kuma shows the last 100 beats and the 24 hour uptime per monitor (`server/routers/status-page-router.js:85-101`), and the page is "maintenance" when any monitor's latest beat is maintenance (`src/pages/StatusPage.vue:794-819`). Kuma has no daily bars and no page timezone.

## Choice

1. **Uptime.** `up / (up + down)` counted in heartbeats, with pending counted as down and maintenance excluded and counted on its own. The contract proposed pending as up; Kuma counts it as down, so v1per follows Kuma. A window with no up or down beats has no uptime and shows "no data"; v1per does not copy Kuma's fallback to the last bucket, which only affects paused or idle monitors.
2. **Windows.** 24 hours, 7 days, 30 days and 1 year use Kuma's buckets: 1440 minutes, 168 hours, 30 UTC days and 365 UTC days, each including the current bucket.
3. **Latency.** Average, minimum and maximum over up beats with a latency. An imported row maps to `latency_sum = ping * up` and `latency_count = up`, which reproduces Kuma's weighted merge exactly. Imported `extras.maintenance` becomes the rollup's maintenance count; Kuma does not count pending separately, so imported pending stays inside `down`.
4. **Retries.** Each probe applies Kuma's retry rule to its own results: pending while `retries < maxretries`, then down; reset on success; the next check after a pending result waits `retry_interval` if it is above 0. `retry_only_on_status_code_failure` is honoured for `json-query`.
5. **Upside down.** Inverts up and down before retries, so an inverted failure retries like any failure and an inverted success resets retries. Pending and maintenance are never inverted.
6. **Important beats and notifications.** v1per keeps Kuma's two tables: a beat is important on the first beat and on up/down, pending to down and any change into or out of maintenance; a notification goes out on a first beat that is down, up to down, pending to down, down to up and maintenance to down. Leaving maintenance to up is silent. `resend_interval` counts consecutive non-important down beats.
7. **Push.** The deadline is the latest beat plus `interval` plus 1 s. A missed deadline records a failure through the retry rule: pending beats every `retry_interval`, then down, then a down beat every `interval` until a push arrives. A push with `status=up` (or no status) is up, any other value is down, both before upside down is applied. An upside-down push monitor is up while pushes are missing.
8. **Groups.** The contract's rule holds, with Kuma's details: direct members only, by their latest monitor-level status; paused members are skipped; a member with no status yet is pending; maintenance members count as up; an empty group is pending. A down member is a failure that goes through the group's own retries and upside down. Groups are evaluated on their own interval, as in Kuma, so group histories keep one beat per interval.
9. **Maintenance.** Heartbeat status 3 means the monitor or an ancestor group was in an active maintenance window and the check was not run. The beat message is "Monitor under maintenance". Push requests during maintenance are stored as maintenance. Window states and strategy semantics follow Kuma's six strategies as listed in the evidence.
10. **Manual monitors.** Pending until an operator sets a status, then that status, recorded every interval so they count in uptime like Kuma. Deviation (team decision): no beat is recorded while no status is set, because a pending beat counts as down and would show a fresh manual monitor as 50 % up with a red bar.
11. **Page timezones with non-hour offsets.** Day bars use hour rollups, and the page's day boundary is the local midnight with the offset's minutes dropped (India +05:30 uses +05:00, Nepal +05:45 uses +05:00, Newfoundland -03:30 uses -03:00). The tooltip shows the boundary used. Minute rollups are not used because their retention is still open in 0002 and imported Kuma data has minute rows for the last 24 hours only, so minute-based bars would change shape at the import boundary.
12. **Probe offline. Proposed, awaiting Mike.** Keep the contract's recommendation: a probe is offline after three missed intervals and leaves the quorum denominator; with no live probe the monitor is pending and a probe offline event goes to the probe's node. Kuma has a single check location, so there is no Kuma rule to match.
13. **Public bar coloring. Proposed, awaiting Mike.** Keep the contract's recommendation: color a day by uptime thresholds (100 % operational, at least 99 % degraded, below 99 % partial outage, below 95 % major outage) and let published incidents override. Kuma has no daily bars to match; its page shows raw beats and the 24 hour uptime.

## Rejected alternatives

- Pending counted as up (the contract's proposal): numbers would rise at cutover for any monitor with retries, and imported rows cannot be split because Kuma folds pending into `down`.
- Copying Kuma's empty-window fallback: it reports a stale bucket as the current window's uptime.
- Event-driven group evaluation: faster, but it changes how many group beats are counted and so the group's uptime compared with its Kuma history.
- Minute rollups for non-hour timezones: depends on an open retention decision and breaks at the import boundary.
- Rounding non-hour offsets to the nearest hour: +05:45 would round to +06:00 while +05:30 has no nearest hour; dropping the minutes is one rule for every zone.

## Additions

- Milestone 06 (team decision): rounds with several probes and late agent results follow the monitoring contract's "Probe and monitor status" section. The lead probe is the live probe with the lowest id.

## Consequences

- Rollups count pending inside down for uptime but keep a pending count of their own for native data, so v1per can show pending separately later without changing the uptime number.
- The monitor-level series records one status per check round, so its counts stay comparable with Kuma's one beat per interval.
- Importers must read `extras` JSON for maintenance counts and expect minute rows for 24 hours and hour rows for 30 days only.
- Notification tests (07) use the important-beat and notification tables as their truth table.
- Items 12 and 13 can change after Mike's answer without touching items 1 to 11.

## Affected contracts

Monitoring contract, `docs/contracts/kuma-notifications.md`, `fixtures/kuma-2.5.0`, milestones 05, 06, 07, 08, 09, 10.
