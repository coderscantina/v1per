# 0013 Status page subscribers, public response times and incident pages

Status: accepted by Mike's request on 2026-11-11 and 2026-11-12. Two points below are proposed and wait for him.

## Problem

Decision 0004 kept page subscribers out of the first release and gave visitors no response times. Status pages should do more than OpenStatus and similar tools: visitors follow a page, see how fast a service answers from where, read what happened after an incident, and can check how an uptime number was counted.

## Choice

- **Response times for visitors** are a page setting, off by default. A second setting adds one series per probe location, named by city and country. Probe names, providers and IDs stay internal.
- **Ranges and percentiles.** `response-times.json` answers 24 hours, 7, 30 and 90 days with p50, p95 and p99 and the incidents and maintenance of the range. Percentiles read raw heartbeats, which the hourly retention pass rewrites once per closed day sorted by monitor. This amends decision 0002 and is proposed there.
- **Subscribers** are a page setting, off by default. Visitors subscribe by email with double opt-in, or follow Atom, RSS and an iCalendar feed. Stored per subscriber: address, locale, component selection, two dates and a token hash. No client address.
- **Webhooks are added by page managers, not visitors**, so nobody anonymous makes the server call an address.
- **Subscriber notices have their own queue tables.** The notification outbox is tied to a node and a channel per row.
- **Incidents** get a page of their own, a postmortem once resolved, and link previews. A gated page serves no preview tags.
- **Uptime figures** carry their check counts, and the page explains the rules it applies.

## Rejected

- Visitor-registered webhooks: an open way to make the server call arbitrary URLs.
- Approximate percentiles from a new rollup column: cheaper to read, not exact, and a storage schema change.
- A rendered preview image per incident: needs an image renderer for little gain.

## Proposed, waiting for Mike

- The amendment to decision 0002 (status pages read compacted raw heartbeats for percentiles).
- An outbound address policy for webhooks. None exists today, for page webhooks or for webhook notification channels.

## Affected contracts

[Status page API](../contracts/status-page-api.md), [capability matrix](../contracts/capability-matrix.md), [decision 0002](0002-storage-split-and-retention.md), [decision 0004](0004-status-pages-and-incidents.md).
