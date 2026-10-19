# Milestone 01: contracts, fixtures and sources

## Outcome

Every later milestone can start from pinned sources, a settled monitoring contract, a ported architecture patterns document and a synthetic Kuma 2.5.0 database that CI can import.

## Dependencies and ownership

None. Integration owns `docs/contracts/`, `sources/` and `scripts/fetch-sources.sh`. Storage and import owns `fixtures/kuma-2.5.0/`. Monitoring reviews the monitoring contract rules.

## Work

1. Finish `sources/manifest.json`: confirm the Kuma 2.5.0 commit (the recorded SHA may be the tag object), add license hashes, and port holocron's `scripts/fetch-sources.sh` with the `V1PER_` prefix. Holocron stays `distributable: false`.
2. Port holocron's `docs/contracts/architecture-patterns.md` to `docs/contracts/architecture-patterns.md`, replacing `SiteScope` with `NodeScope` and analytics examples with monitoring ones. Update the briefing's references to point at it.
3. Read Kuma 2.5.0's uptime calculator, group monitor, push and maintenance code and settle every "Settle in 01" rule in the [monitoring contract](../monitoring-contract.md). Record the result in a decision.
4. Write `fixtures/kuma-2.5.0/`: a generator that builds a Kuma 2.5.0 SQLite file from the pinned schema (template database, legacy patches, 54 knex migrations) and fills it with deterministic data: every monitor type, group nesting, every core provider plus a few others, custom email and Discord templates, all six maintenance strategies, status pages with groups, passwords and custom domains, incidents, tags, proxies, a user with TOTP, API keys, and heartbeats plus `stat_*` rows for 400 days. Record expected counts in a manifest.
5. Record the Kuma notification `config.type` strings and the config fields of email (smtp) and Discord, including template variables, in `docs/contracts/kuma-notifications.md`.
6. Draft the protocol envelope, error shape and capability names for nodes, monitors, pages and incidents, adapted from holocron.

## Acceptance criteria

- `scripts/fetch-sources.sh` fetches and verifies every distributable source.
- The monitoring contract has no "Settle in 01" items left, and each settled rule cites Kuma source lines.
- The fixture generator is deterministic: two runs produce identical files, and the manifest counts match.
- The architecture patterns contract stands on its own without links to Mike's machine.

## Evidence and handoff

Publish the fixture manifest, the settled decision and the source pins in `docs/evidence/m01-contracts-fixtures-and-sources.md`. Hand the fixture to 10 and the envelope to 03.
