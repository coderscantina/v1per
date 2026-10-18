# Monitoring contract

This document owns the meaning of statuses, uptime numbers, incident impact and page colors. Rules marked **Confirmed** were agreed with Mike. Rules marked **Settle in 01** are proposals that milestone 01 confirms against Kuma 2.5.0 source and records in a decision before dependent work starts.

## Heartbeat status

Confirmed: v1per keeps Kuma's four heartbeat statuses so imported history needs no translation.

| Value | Status      | Meaning                                                                 |
| ----- | ----------- | ----------------------------------------------------------------------- |
| 0     | down        | The check failed after its retries                                      |
| 1     | up          | The check passed                                                        |
| 2     | pending     | The check failed but retries are not used up                            |
| 3     | maintenance | A maintenance window covered the monitor; the check result is recorded but not alerted |

A heartbeat row carries monitor, probe, time (UTC, millisecond precision), status, latency in milliseconds, message, retry count, and for http types the status code and certificate expiry. Imported Kuma heartbeats carry the pseudo probe `kuma-import`.

## Probe and monitor status

- Each probe evaluates its own retries per Kuma's `maxretries` and `retry_interval`, producing a probe status.
- Confirmed: the monitor status follows its quorum rule over live probes. `majority` (default) means down when at least half of the live probes report down. `any` means down when one does. `all` means down only when all do.
- Settle in 01: a probe is offline after three missed intervals and leaves the quorum denominator. With no live probe the monitor is pending and a probe offline event goes to the probe's node.
- Push monitors have no probe. The server marks them down when no push arrives within the interval plus retries, like Kuma.
- `upside_down` inverts the check result before retries, like Kuma.
- Group monitors are down when any member is down, pending when any member is pending and none is down. Settle in 01: compare with Kuma's group rules.
- Manual monitors take the status an operator sets.

## Uptime

- Confirmed: imported `stat_minutely`, `stat_hourly` and `stat_daily` rows are kept as they are, so 24-hour, 30-day and 1-year uptime match Kuma for imported periods.
- Settle in 01: for new data, uptime is `up / (up + down)` counted in heartbeats, with pending counted as up and maintenance excluded. Check this against Kuma's uptime calculator and adopt Kuma's rule if it differs, so numbers do not jump at cutover.
- With several probes, uptime counts the monitor status, not each probe. Per-probe uptime is shown to grant holders only.
- Rollups store up, down, pending and maintenance counts and min, max and average latency per monitor per minute, hour and day (UTC). Day bars in a page timezone are built from hourly rollups. Settle in 01: page timezones with non-hour offsets (India, Nepal) use minute rollups or are rounded; record which.

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

Settle in 01: whether a short down blip with no published incident colors a public daily bar, or only lowers the uptime number. Recommendation: color the bar by uptime thresholds (100 % operational, at least 99 % degraded, below that partial outage, below 95 % major outage) and let published incidents override.
