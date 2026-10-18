# Milestone 05: checks and the built-in probe

## Outcome

A member creates monitors on a node, the built-in probe runs them on schedule, results land in DuckDB, and the admin UI shows live status, history and latency. Kuma push URLs record heartbeats.

## Dependencies and ownership

Requires 02 and 04. Monitoring owns `checks`, `probes` (built-in part) and `monitors`. Frontend owns the monitor list, editor and detail screens.

## Work

1. Implement check execution for http, keyword, json-query, port, ping, dns and TLS certificate expiry, with Kuma's options for each (methods, headers, body, auth, accepted status codes, redirects, ignore TLS, upside down, timeouts). Proxies per the open decision in the index.
2. Implement push, group and manual monitors, and `ALL /api/push/{token}` matching Kuma 2.5.0 parameters and responses.
3. Build the scheduler in the built-in probe: per-monitor intervals with stable jitter, retries, bounded concurrency, no drift under load.
4. Implement monitor evaluation per the [monitoring contract](../monitoring-contract.md): retries, pending, quorum over probes (single probe for now), transitions journal in SQLite, topic events.
5. Admit results through the storage interface from 02.
6. Build the UI: monitor list per node with live status, editor per type with sealed secret fields, detail page with uptime numbers, latency chart and heartbeat list.

## Acceptance criteria

- Each type passes and fails against local test servers, including timeouts, redirects, bad certificates and keyword misses.
- A push with Kuma's query parameters behaves like Kuma, and a missing push marks the monitor down on time.
- At 2,000 monitors on the reference budget, under 0.1 % of checks start more than one interval late.
- Status changes reach a subscribed browser within the target.

## Evidence and handoff

Type matrix, scheduler measurements and screenshots in `docs/evidence/m05-checks-and-built-in-probe.md`. Hand the transitions journal to 06, 07 and 08.
