# Milestone 01 evidence: contracts, fixtures and sources

Recorded October 18, 2026, on an Apple M1 Pro (macOS 27.0.1), Rust 1.99.1.

## Run commands

```sh
bash scripts/fetch-sources.sh
cargo test -p v1per-kuma-fixture
```

## Results

| Criterion | Result |
| --- | --- |
| `scripts/fetch-sources.sh` fetches and verifies every distributable source | Passes. Kuma 2.5.0 at `d9a60dfc` (lightweight tag, commit confirmed with `git ls-remote`) and OpenStatus at `228495ca` verify commit and license sha256. Holocron is skipped as not distributable. |
| No "Settle in 01" items left; settled rules cite Kuma source lines | Passes. [0008](../decisions/0008-monitoring-rules.md) settles uptime, windows, retries, upside down, important beats, push deadlines, groups, maintenance and timezone rounding with file:line references. Items 12 (probe offline) and 13 (public bar colors) are proposals awaiting Mike. |
| Fixture generator is deterministic and matches its manifest | Passes. 4 tests: two runs give identical output, counts match `fixtures/kuma-2.5.0/manifest.json`, schema matches `schema.sql`. |
| Architecture patterns contract stands on its own | Passes. [architecture-patterns.md](../contracts/architecture-patterns.md) has no links to Mike's machine and describes the SSE transport holocron actually uses. |

The fixture covers all 33 Kuma monitor types, 13 channels (all core providers plus three others, custom email and Discord templates), all six maintenance strategies, three status pages and about 194,000 heartbeats. Its schema was taken from Mike's Kuma 2.5.0 database (schema only) and cross-checked against the 54 knex migrations. No value from Mike's database appears in the repository.

The notification parity contract is [kuma-notifications.md](../contracts/kuma-notifications.md).

## Not verified

- liquidjs template output was read from source, not executed, because Kuma's dependencies are not installed.
- Push pings in the fixture are integers only.

## Changed behavior

- Uptime counts pending as down, following Kuma; the briefing's first proposal counted it as up.
- Heartbeat status 3 means the check was skipped for maintenance, not recorded.

## Open items

- Mike: 0008 items 12 and 13, and domain expiry alerts (see the [milestone index](../milestones/README.md#open-decisions)).
