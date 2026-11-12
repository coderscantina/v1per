# Milestone 14 SLAs evidence

Measured on 2026-10-23 for the closing verification. Design in [0012](../decisions/0012-slas.md), scope in [milestone 14](../milestones/14-slas.md).

Verdict: every acceptance criterion passes. The workspace tests cover each one, the new browser journey covers the admin, status page and client paths, and an upgrade from 26.10.20 on Mike's real Kuma data keeps every row. Two findings stay open: on imported days the SLA and the day bars count a month's edge a little differently, and the closed-period labels on a status page collide on phones. Both are listed under follow-ups.

## Machine and versions

| Item | Value |
| --- | --- |
| Machine | Apple M1 Pro laptop, macOS 27.0.1. Not the reference host |
| Toolchain | rustc 1.99.1, bun 1.4.4, Playwright 1.63.0 with its Chromium |
| Source | `sla/verify` from `feature/slas` at `c625548`, plus the journey and the fix in this commit |
| Release for the upgrade | 26.10.20 at `da7bf8e`, control schema 13 |
| Builds | debug builds of both trees, run from copies in `/tmp` |
| Real data | `kuma/kuma.db` of 2026-10-18, copied to `/tmp` and read there |

## Checks

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --no-fail-fast` | 311 passed, 2 ignored (the 2,000-monitor scheduler load run and a doc example) |
| `cargo run -p v1per-protocol --bin export-types -- --check` | up to date, 368 files |
| `bun run typecheck`, `bun run lint`, `bun run test` | clean, 152 tests |
| `bun run build`, `cargo build -p v1per-server -p v1per-probe` | built |
| `bun run e2e` with `V1PER_BIN` set to the feature build | 54 passed in 4.9 min, the 4 SLA tests included |

The new journey `apps/web/e2e/slas.spec.ts` runs four tests:

1. An admin adds an SLA with an availability target, major outage times, Monday to Friday business hours and a holiday. The list shows its targets, and the period view shows the live period with the incident on its monitor.
2. The admin picks it in the page's SLAs tab. The internal snapshot has the per-monitor rows. The public snapshot has the aggregates, no `monitors` and no incident title, and the page shows the section.
3. A client invited as Admin on the SLA's own node sees the SLA and its period without add, edit or delete controls, gets the read-only notice in the editor, and the server answers `403` to `slas.delete`.
4. List, editor, period view, report in print emulation and status page section at 1440 and 390 px, light and dark, with axe and overflow checks.

Test 4 failed at first: axe refused the period view's commitments, a `<dl>` whose children wrapped the `dt` and `dd` in further `div`s next to a badge and a progress bar. `SlaPeriodPage.vue` now lays the cards out in a plain grid and gives each figure its own `<dl>`. It looks the same. In the full suite the journey first timed out because `getByLabel('Name')` also matched a tree button of a node another journey had renamed; it now matches the label exactly.

## Acceptance criteria

| Criterion | Result | Evidence |
| --- | --- | --- |
| Availability matches the page uptime for the same monitor and period in `each` mode | pass | `each_mode_availability_equals_the_page_uptime`, `period_totals_equal_the_page_uptime`. On real imported data see the comparison below |
| `any_down` counts each down minute once; a day's tally survives deleting its minute rollups | pass | `down_minutes_count_each_minute_once` (overlapping and disjoint outages, pending as down), `a_tally_outlives_the_minute_rollups_of_its_day` |
| Clocks skip nights, weekends and holidays across DST; a self-healed incident is not a missed response | pass | `business::tests` (`counts_only_open_hours`, `deadline_carries_over_the_weekend`, `holidays_are_closed`, `spring_forward_shortens_the_day`, `fall_back_lengthens_the_day`), `business_hours_skip_a_weekend`, `a_self_healed_incident_is_not_a_missed_response`, `self_healed_incident_needs_no_response` |
| A frozen period keeps its result after its targets change and a covered monitor is deleted, and changes only through recompute | pass | `a_frozen_period_changes_only_through_recompute` (updates the terms, deletes a monitor, a client Admin's recompute is refused, staff recompute applies the new terms) |
| A client Admin reads and every write is refused; a sibling Viewer cannot tell it exists | pass | `clients_read_their_sla_and_siblings_cannot_tell_it_exists`; e2e test 3 |
| A public page shows aggregates and no incident title, draft or internal field; public and internal snapshots tested | pass | `the_public_snapshot_shows_sla_aggregates_only`, `the_internal_snapshot_has_per_monitor_rows`, `an_sla_not_selected_on_the_page_is_absent`; e2e test 2 |
| The 75 % warning and each breach kind are sent once per period, no duplicate after a restart | pass | `each_alert_is_queued_once_per_period_across_a_restart`, `a_response_deadline_after_the_freeze_is_alerted_once`, `a_latency_breach_waits_for_the_period_to_close`, `a_per_monitor_breach_carries_its_monitor`, `a_running_period_is_at_risk_from_three_quarters_of_the_budget` |
| An SLA started before the Kuma cutover shows imported history in `each` mode and no data for `any_down` days without minute rows | pass | Real data rehearsal below |

## Upgrade rehearsal on real data

The 26.10.20 binary initialised a data directory, imported the Kuma copy into `Root` in 7.9 s (69 monitors, 134,694 raw heartbeats, 92,037 minute, 49,034 hour and 11,652 day stats, uptime matching Kuma for all 69), and served for about two minutes, which added 118 heartbeats. Channels were switched off before the first `serve`. The directory was then copied three times.

| | 26.10.20 | `v1per upgrade --backup-to` | `serve --migrate` |
| --- | --- | --- | --- |
| Control schema | 13 | 14 | 14 |
| Inbox and heartbeat schema | 1 and 1 | 1 and 1 | 1 and 1 |
| Monitors | 69 | 69 | 69 |
| Incidents, incident updates | 1, 1 | 1, 1 | 1, 1 |
| Outbox | 0 | 0 | 0 |
| Nodes, users, pages | 19, 2, 4 | 19, 2, 4 | 19, 2, 4 |
| Channels (active) | 1 (0) | 1 (0) | 1 (0) |
| Raw heartbeats | 134,812 | 134,812 | 134,812 |
| Minute, hour, day rollups | 134,898, 2,684, 339 | same | same |
| Kuma stats rows | 152,723 | 152,723 | 152,723 |
| Time | | 0.81 s, with a 9.6 MB backup | 0.67 s from start to the first healthy `/health` |

Plain `serve` on the third copy exits with code 3 and leaves the data at schema 13:

```
error: upgrade_required: the data is older than this release; run `v1per upgrade --backup-to <file>`, or start with --migrate to upgrade without a backup
```

### SLAs over imported history

On the upgraded copy, two SLAs over the same six imported monitors (Europe/Vienna, monthly, contract start 2026-06-01, 99.9 %, 800 ms, business hours) were created through the API, one per combine mode. The job froze June to September for the `each` SLA 0.14 s after creation, and tallied 144 days and froze the same months for the `any_down` SLA within 3.8 s.

- `each` reads imported history. June: lowest monitor 99.896 %, missed with 104 % of the budget used. September: 99.965 %, met.
- `any_down` shows no data for June to September. Of 144 tallies only 18 October counts minutes (928), the day Kuma's 24 hours of minute rows cover from local midnight on. 17 October is no data because those rows start at 13:26 UTC, after the day began. 19 to 22 October are no data because nothing ran between the Kuma copy and the import. Imported raw beats older than that are probe-level only and count for nobody's minutes.

September availability against the monitor page's day bars (Europe/Vienna):

| Monitor | Interval | SLA | Day bars | Difference |
| --- | --- | --- | --- | --- |
| 56 | 60 s | 99.993036 %, 3 down of 43,080 | 99.993056 %, 3 down of 43,200 | 120 beats |
| 35 | 15 min | 99.965181 %, 1 down of 2,872 | 99.965278 %, 1 down of 2,880 | 8 beats |
| 2, 3, 4, 32 | 60 s to 5 min | 100 % | 100 % | none |

Both differences are two hours of beats, the CEST offset. Kuma kept day rows from April on, but hour rows only from 18 September 13:00 UTC. So 1 to 18 September exist only as UTC days, and the bars flag those 18 days as `utc_approximate`. The SLA and the bars cut the month's start and the hand-over day differently. The outcome is the same. October, which hourly and native data cover, matches exactly. This is the import cutover difference storage reported might happen. It disappears for months after the import.

A third SLA, over two of those monitors with 99.97 % and 130 ms, gave the at-risk (latency in the running month) and missed (every closed month) states for the visual check. The public snapshot of an imported status page then carried both SLAs with their history and no per-monitor rows.

## Screenshots

`docs/evidence/m14/` holds the journey's screenshots on synthetic data: `sla-list`, `sla-editor`, `sla-period`, `sla-report-print` and `sla-status-page`, each at `desktop` and `phone` in `light` and `dark`.

The at-risk and missed states were checked on the real data at the same sizes and schemes: period view, report in print emulation, list, editor and status page section. Those screenshots are not committed because they show client names from Mike's Kuma database.

## Not verified

- Alerts to a real channel. Channels stayed off on real data; the outbox tests check queuing, templates and once-per-period.
- The at-risk and missed states in committed screenshots. The synthetic journey has no history to miss, and the seeded demo has no SLAs.
- Frozen periods on a real month boundary: the rehearsal froze past months during backfill, not a month closing while `serve` runs. The job tests cover the hour delay.
- The release build and Docker. Both trees ran as debug builds.
- The milestone docs, [0012](../decisions/0012-slas.md) and [milestone 14](../milestones/14-slas.md), are still uncommitted on `main`, so their links resolve only once they land.

## Follow-ups

- On a phone the status page's closed-period labels overlap with four to about six periods ("Jun 2026Sep 2026" at 390 px). `SlaHistory.vue` shows the oldest label from four periods on, whatever the width.
- On a phone the period view's edit button wraps onto a line of its own below Report, CSV and Recompute.
- Building two worktrees in one `CARGO_TARGET_DIR` reused stale workspace crates, because both trees hash to the same package IDs and the artifacts had newer timestamps. `cargo test` and `cargo build` failed to compile until `cargo clean` ran for the workspace packages; a build that fails silently leaves the old binary in place. Separate target directories per release tree avoid it.
