# Milestone 08: incidents and maintenance

## Outcome

Members run an incident from first report to resolution with a timeline and per-component impact, failing monitors open auto-incidents, and planned maintenance shows up and silences alerts.

## Dependencies and ownership

Requires 05 and 07. Monitoring owns `incidents`. Frontend owns the incident and maintenance screens.

## Work

1. Implement incidents per [0004](../decisions/0004-status-pages-and-incidents.md): node, title, affected monitors and static components, updates with state, message and per-component impact, publication per page.
2. Implement auto-incidents: one open auto-incident per monitor, opened on a down transition, resolved on recovery unless edited.
3. Implement maintenance with Kuma's six strategies (single, recurring interval, weekday, day of month, cron, manual) and timezones, covering monitors and appearing on pages.
4. Feed maintenance into monitor evaluation (status 3) and notification suppression.
5. Build the UI: incident list and editor with a timeline, quick update, auto-incident triage (publish, edit, dismiss), maintenance list and editor with a preview of the next windows.

## Acceptance criteria

- The impact rules in the [monitoring contract](../monitoring-contract.md) hold in tests, including "latest update wins".
- A monitor that fails twice in a row while its first auto-incident is still open does not open a second one.
- Every maintenance strategy produces the expected windows across DST changes.
- Members can manage incidents in their subtree; Viewers can only read them.

## Evidence and handoff

Schedule tests and screenshots in `docs/evidence/m08-incidents-and-maintenance.md`. Hand incidents and maintenance to 09.
