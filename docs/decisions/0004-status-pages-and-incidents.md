# 0004: Status pages and incidents

Status: Adopted by Mike on 2026-10-18.

## Problem

Kuma's status pages have one free-text banner per page and no incident lifecycle. Clients and the public need a page that explains outages; teams need a workflow that starts from monitor failures.

## Constraints

Pages for the public, for clients and for teams. Auto-incidents from monitors. Scheduled maintenance. No page subscribers in the first release. OpenStatus is a model, not a code source.

## Choice

**Pages**

- A page lives on a node. Its components are picked from monitors anywhere in that node's subtree, plus static components with no monitor. Components sit in ordered groups.
- Access: public, or restricted with any combination of node grant login, page password, magic link (allowed emails or email domains) and share tokens. A team-internal page is restricted to grant login.
- Grant holders see response-time charts and per-probe detail; other viewers see status, bars, uptime and published incidents only.
- Pages are served at `/s/{slug}` and on claimed hostnames routed by `Host`. TLS is the reverse proxy's job.
- A separate light Vite entry renders pages. The server injects the initial JSON; a per-page SSE stream follows.
- Components show 90 daily bars in the page timezone and the 90-day uptime. Badges are SVG, for public pages and share tokens.
- Each page has a default locale and enabled locales (en, de).

**Incidents**

- One incident entity for manual and automatic cases, unlike OpenStatus's three. It belongs to a node and names affected monitors and static components.
- Updates carry a state (investigating, identified, monitoring, resolved), a message and an impact per component. The latest update that names a component wins.
- An auto-incident opens on a monitor's down transition. At most one open auto-incident per monitor.
- Publication is per page. Each page sets its auto-incident mode: `draft` (internal until someone publishes it to the page) or `auto_publish` after a grace period. Manual incidents are published to the pages that show their components, and the author can remove pages.

**Maintenance**

- Kuma's six strategies: single, recurring interval, weekday, day of month, cron, manual, with timezones.
- A window covers monitors, marks their heartbeats as maintenance, suppresses alerts and shows on pages that show those monitors.

## Rejected alternatives

- Pages spanning several nodes: the subtree rule covers the cases and keeps access simple.
- Always auto-publishing or never: Mike wants the choice per page.
- Kuma-style recent-heartbeat bars: daily bars carry incidents and maintenance and read better for clients.

## Consequences

Page reads must be built per audience and tested so no internal field or draft incident reaches a public or client viewer. Subscribers can later attach to the publication step without changing incidents.

## Affected contracts

Monitoring contract (impact and colors), protocol page and incident types, milestones 08, 09, 10.
