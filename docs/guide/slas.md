# SLAs

An SLA records what you promised a client: availability, how fast you respond to and resolve incidents, and average latency. V1Per tracks each period against those targets, warns before and when a target is missed, and gives you a report per period.

## Who can set them up

An SLA belongs to a node and covers monitors from that node and the nodes below it. Creating, editing and recomputing an SLA needs the Admin or Owner role on a node above the SLA's node, or the instance admin. Everyone with a role on the node or above can read it.

So a client's own Admin can read their SLA but never change it. Put a client's SLAs on the client's top node: a client Admin on `autohaus` can edit SLAs on `schmidt` below it, but not on `autohaus` itself.

Open a node's **SLAs** tab under Configure to see its SLAs. Click a name to see its periods.

## Terms

| Field | What it does |
| --- | --- |
| Period | Monthly, quarterly or yearly, in the SLA's timezone |
| Contract start | The first period starts on this day. Earlier periods are not computed |
| Timezone | Days, periods and business hours count in this zone |
| Availability across monitors | **Each monitor**: every covered monitor has to meet the target on its own. **As one service**: the service is down in any minute in which one covered monitor is down |
| Covered monitors | Monitors from this node and the nodes below. Static components are not covered |
| Availability | A percentage such as 99.9 |
| Average latency | Milliseconds, the average of successful checks per monitor |
| Incident times | Response and resolution targets per impact: major outage, partial outage and degraded |
| Business hours | Weekly hours and holidays for the incident clocks. Without hours they run around the clock |
| Alert channels | Channels from this node or the nodes above that get SLA alerts |

Every target is optional. Leave a field empty for no commitment.

## How it is measured

- **Availability** uses the same numbers as the uptime on status pages: checks that were up against checks that were down, pending counted as down, maintenance left out. It always counts around the clock.
- **As one service** counts minutes: a minute is down when any covered monitor had a down or pending check in it. V1Per keeps a daily tally per SLA, so this works for long periods without keeping every minute.
- **Incidents** count when they name a covered monitor and started in the period, drafts included. The target comes from the worst impact the incident reached.
- **Response** runs from the incident's start to the first update a person writes. An incident that resolves before its response deadline without an update needed no response.
- **Resolution** runs from the start until the incident is resolved.
- Both clocks count only business hours when the SLA has them. Maintenance does not stop them.
- **Latency** is the average of successful checks of http, keyword, json-query, port, ping and dns monitors. It is judged when the period ends; while it runs, an average above target shows as at risk.

A period shows **No data** where there is nothing to measure, for example before the first check. No data never counts as missed.

## Periods

The SLA view shows the running period live and every closed period since the contract start. Each commitment shows on track, at risk, met or missed. Availability is at risk once 75 % of the downtime budget is used, and missed once all of it is.

A period is frozen one hour after it ends. Later changes to the SLA, its monitors or incidents do not change a frozen period. **Recompute** on a closed period computes it again from the data V1Per still has. Recompute needs the same role as editing.

Creating an SLA with an earlier contract start computes the past periods from the history V1Per has, including history imported from Uptime Kuma. **As one service** needs minute data, so imported periods before the cutover show no data there.

## Reports

**Report** opens a printable page for a period with the terms, the commitments and the incidents. Print it or save it as PDF from the browser. **CSV** downloads the period's monitors and incidents for a spreadsheet.

## On status pages

In a status page's **SLAs** tab, pick which SLAs the page shows, in order. A page can show SLAs from its own node and the nodes below it.

The page shows each SLA's running period and its last 12 closed periods with targets, results, the share of incidents within target and the average times. Visitors never see incident titles or drafts here. Team members signed in to the page also see the result per monitor the page shows.

## Alerts

SLA alerts go to the SLA's alert channels, once per period and kind:

| Alert | When |
| --- | --- |
| SLA budget | 75 % of the downtime budget is used |
| SLA availability | The availability target is missed |
| SLA response | An incident passed its response deadline without an update from a person |
| SLA resolution | An incident passed its resolution deadline |
| SLA latency | A closed period's average latency was above target |

An incident whose deadline falls after its period was frozen still sends its alert. Periods that ended before the SLA was created send nothing. Alerts are written in the default language of the first status page that shows the SLA, otherwise in English.
