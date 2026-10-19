# Monitoring contract

This document owns the meaning of statuses, uptime numbers, incident impact and page colors. Rules marked **Confirmed** were agreed with Mike. Rules marked **Settled** were checked against Kuma 2.5.0 source in milestone 01; [decision 0008](decisions/0008-monitoring-rules.md) holds the evidence with Kuma file and line references. Rules marked **Proposed** wait for Mike.

## Heartbeat status

Confirmed: v1per keeps Kuma's four heartbeat statuses so imported history needs no translation.

| Value | Status      | Meaning                                                                 |
| ----- | ----------- | ----------------------------------------------------------------------- |
| 0     | down        | The check failed after its retries                                      |
| 1     | up          | The check passed                                                        |
| 2     | pending     | The check failed but retries are not used up                            |
| 3     | maintenance | A maintenance window covered the monitor or an ancestor group; the check was not run and nothing is alerted |

A heartbeat row carries monitor, probe, time (UTC, millisecond precision), status, latency in milliseconds, message, retry count, and for http types the status code and certificate expiry. Imported Kuma heartbeats carry the pseudo probe `kuma-import`.

## Probe and monitor status

- Settled: each probe evaluates its own retries like Kuma. A failed check is pending while `retries < maxretries`, then down; the counter resets on success. After a pending result the next check waits `retry_interval` when it is above 0, otherwise `interval`. For `json-query` with `retry_only_on_status_code_failure`, a failed JSON query is down at once.
- Confirmed: the monitor status follows its quorum rule over live probes. `majority` (default) means down when at least half of the live probes report down. `any` means down when one does. `all` means down only when all do.
- Proposed, awaiting Mike: a probe is offline after three missed intervals and leaves the quorum denominator. With no live probe the monitor is pending and a probe offline event goes to the probe's node. Kuma has a single check location, so there is no Kuma rule to match.
- Adopted (milestone 06, team decision): a monitor-level entry is recorded for each result of the lead probe (the live probe with the lowest id) and for any result that changes the monitor status, so uptime is not multiplied by the number of probes. Results an agent delivers more than one interval late (after an outage) count only in that probe's own series; the monitor series keeps a gap rather than a guessed status.
- Settled: push monitors have no probe. The deadline is the latest beat plus `interval` plus 1 s. A missed deadline records "No heartbeat in the time window" through the retry rule: pending every `retry_interval`, then down, then down every `interval` until a push arrives. `status=up` or no status is up, any other value is down.
- Settled: `upside_down` inverts up and down before retries, so an inverted failure retries and an inverted success resets retries. Pending and maintenance are never inverted. An upside-down push monitor is up while pushes are missing.
- Settled: group monitors read the latest monitor-level status of their direct members. Down when any member is down, pending when any member is pending or has no status yet and none is down, up otherwise. Paused members are skipped, members in maintenance count as up, an empty group is pending. A down member goes through the group's own retries and upside down. Groups are evaluated on their own interval.
- Settled: a monitor is in maintenance when one of its windows is active or an ancestor group is in maintenance. Its check is skipped and the beat is status 3 with message "Monitor under maintenance"; a push during maintenance is stored as status 3.
- Settled: manual monitors show pending until an operator sets a status, then take that status and record a beat every interval. Until a status is set they record no beat, so an unset manual monitor does not count as down in uptime or color its bars (milestone 05 fix; Kuma records pending beats here, which v1per counts as down).
- Settled: a beat is important on the first beat, on up to down, down to up, pending to down and on any change into or out of maintenance. A notification goes out on a first beat that is down, up to down, pending to down, down to up and maintenance to down; leaving maintenance to up is silent. `resend_interval` counts consecutive non-important down beats and repeats the down alert when reached.

## Uptime

- Confirmed: imported `stat_minutely`, `stat_hourly` and `stat_daily` rows are kept as they are, so 24-hour, 30-day and 1-year uptime match Kuma for imported periods.
- Settled: uptime is `up / (up + down)` counted in heartbeats, with pending counted as down and maintenance excluded, as Kuma does. The milestone 01 proposal counted pending as up; Kuma does not, and its rollups fold pending into `down`. A window with no up or down beats shows no data.
- Settled: the 24-hour, 7-day, 30-day and 1-year numbers use Kuma's windows: 1440 minute buckets, 168 hour buckets, 30 UTC days and 365 UTC days, each including the current bucket.
- Raw heartbeats are stored per probe. Rollups are keyed by monitor and probe. The evaluator adds a monitor-level series (probe empty) to the rollups only, never as raw rows; uptime, bars and monitor latency read that series. Per-probe uptime and latency come from the per-probe rollups and are shown to grant holders only. Push, group and manual monitors have no probe, so their raw rows are the monitor-level series. Imported Kuma rows count as the monitor-level series.
- Rollups store up, down, pending and maintenance counts and min and max latency plus latency sum and count (so averages merge exactly), counted over up heartbeats, per minute, hour and day (UTC). For uptime, `down` includes pending; the pending count is kept for display only. An imported Kuma row maps to `latency_sum = ping * up`, `latency_count = up`, and its `extras.maintenance` to the maintenance count. Kuma keeps minute rows for 24 hours and hour rows for 30 days, so imported history is mostly day rows. Where native and Kuma hourly rows do not cover a day, a bar falls back to Kuma `stat_daily` and is marked as a UTC day. Day bars in a page timezone are built from hourly rollups.
- Settled: page timezones with non-hour offsets drop the offset's minutes for the day boundary (India +05:30 and Nepal +05:45 use +05:00). The bar tooltip shows the boundary used.

## Incident impact

Confirmed, from OpenStatus:

- Impact values, ordered: `operational`, `degraded`, `partial_outage`, `major_outage`.
- Every incident update can set the impact per affected component. The latest update that names a component wins.
- Update states: `investigating`, `identified`, `monitoring`, `resolved`. Resolving sets all affected components to `operational`.
- An auto-incident starts at `investigating` with `major_outage` for the failing monitor and resolves when the monitor recovers, unless someone has edited it; then a person resolves it.

## Component color and page status

For a component at a moment, take the worst of:

- monitor status: down maps to `major_outage`, pending to `degraded`, up to `operational`
- the impact of published incidents that name the component
- maintenance, shown as its own state, which overrides monitor status but not a published incident

The page status is the worst component status. A daily bar shows the worst state that day and its uptime; its tooltip lists incidents and maintenance that touched the day. Static components have no monitor and take their color from incidents and maintenance only.

Proposed, awaiting Mike: whether a short down blip with no published incident colors a public daily bar, or only lowers the uptime number. Recommendation: color the bar by uptime thresholds (100 % operational, at least 99 % degraded, below that partial outage, below 95 % major outage) and let published incidents override. Kuma has no daily bars to match.
