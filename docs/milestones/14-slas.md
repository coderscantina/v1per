# Milestone 14: SLAs

## Outcome

Staff set an SLA on a client node with availability, response, resolution and latency targets. Staff and the client follow the running period in the admin app and on an opt-in status page section. Each closed period is frozen with a printable report and a CSV, and the SLA's channels warn before and on a breach, per [0012](../decisions/0012-slas.md) and the [monitoring contract](../monitoring-contract.md#slas).

## Dependencies and ownership

Requires 04, 07, 08 and 09. Milestone 12 waits for it.

- Integration owns migration `0014_slas.sql`, the protocol operations and types, the export and the capability matrix row.
- Monitoring owns the SLA module in `apps/server/src/slas/`: periods, availability, incident clocks, budget, alerts and freezing.
- Storage and import owns the reads from rollups, the daily `any_down` tally job and its retention interplay in `crates/storage`.
- Frontend owns the SLA screens, the report page, the status page section and the locale keys.

## Work

1. Schema: SLAs with monitors, per-impact time targets, weekly hours, holidays and channels, the daily tally, frozen period results and the page selection.
2. Protocol: create, update, delete, list and read SLAs; read a period with its incidents; recompute a frozen period; pick SLAs on a page. Writes check Admin through a strict ancestor grant.
3. Computation: availability in both modes, business-time clocks, worst impact per incident, average latency, downtime budget, and period freezing one hour after close.
4. Backfill from the start date, marking commitments the data cannot support as no data.
5. Alerts: budget warning and the four breach kinds through the outbox, once per period and kind, with English and German templates.
6. Admin app: SLA list on a node, editor, period view with incidents and their times, the printable report and the CSV download.
7. Status page: the SLA section in the public and internal snapshots and the light page entry, with English and German labels.
8. Guide: a page in `docs/guide` on setting up an SLA and what clients see.

## Acceptance criteria

- On a fixture with known heartbeats, availability matches the page uptime for the same monitor and period, in `each` mode.
- `any_down` over overlapping and disjoint outages of two monitors counts each down minute once, and a day's tally does not change when minute rollups for it are later deleted.
- Response and resolution clocks skip nights, weekends and holidays in the SLA's timezone, across a DST change. A self-healed incident with no human update is not a missed response.
- A frozen period keeps its result after its targets change and a covered monitor is deleted, and changes only through recompute.
- A client Admin on the SLA's node reads the SLA and gets a refusal on every write. A Viewer on a sibling client cannot tell it exists.
- A public status page shows the SLA aggregates and no incident title, draft or internal field. The test checks the public and internal snapshots.
- The 75 % warning and each breach kind are sent once per period, and a server restart sends no duplicate.
- An SLA started before the Kuma import cutover shows imported history in `each` mode and no data for `any_down` days without minute rows.

## Evidence and handoff

`docs/evidence/m14-slas.md` with the fixture, test runs, screenshots of the SLA screens, report page and status page section in light and dark at desktop and phone widths, and what was not verified.
