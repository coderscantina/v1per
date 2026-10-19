# Development workflow

Use this document when assigning work, changing shared interfaces or closing a milestone. Product requirements live in [the technical briefing](technical-briefing.md). Acceptance criteria live in [the milestone files](milestones/README.md).

## Begin an assignment

Read the assigned milestone, its dependencies, the decisions it links and the source files involved. State the files the task owns, the interface it consumes, the behavior it adds and the check that will exercise it. Inspect existing implementation before editing. Ask the integration owner to settle overlapping ownership before two agents change the same migration or registry.

Each task produces one usable behavior, such as a failing http monitor opening an auto-incident, a remote agent resuming after a server restart, or a Kuma push URL recording a heartbeat. Keep tasks small enough to review in one pass. A milestone may hold several tasks; its gate requires their integrated behavior.

Briefs are written for an agent that knows nothing about earlier conversations. A brief names its outcome, dependencies, owned files, current code (dated), numbered work, acceptance criteria, checks and handoff. Check that the code survey in a brief still holds before relying on it.

## Roles and module ownership

Four roles own the work. They are responsibilities, not people.

- **Integration**: protocol, HTTP and startup, identity, node tree and access, CLI, gate evidence.
- **Monitoring**: check execution, probes and the agent protocol, monitor evaluation and quorum, notifications, incidents and maintenance logic.
- **Storage and import**: SQLite and DuckDB schemas and migrations, result inbox and transfer, rollups and retention, Kuma import, backup and restore.
- **Frontend**: admin app, status page app, Kit components and tokens, locale catalogs.

| Module               | Owned implementation                                         | Owner              | Interface and test surface                                                   |
| -------------------- | ------------------------------------------------------------ | ------------------ | ---------------------------------------------------------------------------- |
| Protocol             | `crates/protocol`, export tooling                            | Integration        | Wire types, operation maps, typed errors, generated TypeScript and OpenAPI   |
| HTTP and startup     | `apps/server/src/http`, `config.rs`, `state.rs`, `main.rs`   | Integration        | Router, configuration, limits, lifecycle, dependency composition             |
| Identity and access  | `identity`, `nodes`                                          | Integration        | Principals, sessions, 2FA, API keys, sealed secrets, `NodeScope`, capabilities |
| Checks and probes    | `crates/checks`, `probes`                                         | Monitoring         | Typed check results, built-in probe, agent mode, assignment and batch protocol |
| Monitor evaluation   | `monitors`                                                   | Monitoring         | Retries, quorum, push deadlines, transitions journal                         |
| Notifications        | `notifications`                                              | Monitoring         | Outbox, providers, templates, resend rules                                   |
| Incidents            | `incidents`                                                  | Monitoring         | Incidents, updates, auto-incidents, maintenance schedules and suppression    |
| Status pages         | `pages`                                                      | Integration        | Page snapshots per audience, access modes, domains, badges                   |
| Storage              | `crates/storage`, migrations, transfer worker, `maintenance` | Storage and import | Admit batch, apply batch, bounded reads with watermark, rollups, retention, backup |
| Import               | `imports`, `fixtures/kuma-2.5.0`                             | Storage and import | Inspect, dry run, map, seal, reconcile, inventory                            |
| Vue data adapters    | `apps/web/src/api`, `generated`, `services`, `composables`   | Frontend           | Generated resources, Vue Query keys, live snapshot adapters                  |
| Vue experience       | `apps/web/src/admin`, `status`, `components`, `assets`       | Frontend           | Kit-based accessible screens, routing, translations, tokens                  |

Keep deep modules with small interfaces. Pages do not know SQL. Probes do not know how results are stored. SQLite and DuckDB code stays separate because their transactions differ.

## Shared changes

- The integration owner integrates new operations and generated output. A domain owner proposes input, result, errors, authorization and limits with the operation. Generated files are regenerated, never edited.
- The storage owner orders migrations. Every migration states its engine, upgrade behavior and compatibility with pending inbox batches and backups.
- The frontend owner integrates components, tokens, navigation and locale catalogs. Domain tasks use those components instead of local substitutes.
- Server results are cached in Vue Query only. Services own normalized UI state and operations.

## Copying from holocron

Copy holocron code where the briefing says so, then adapt it. Record the holocron path and revision in the commit message and in the brief's handoff. Remove analytics-specific code rather than leaving it unused. Keep Kit license notices. Rename `holocron` and `HOLOCRON_` everywhere. Do not keep absolute paths to Mike's machine in code or docs; link pinned sources instead.

## Decisions before dependent work

Milestone 01 settles the open rules in the [monitoring contract](monitoring-contract.md), the protocol envelope and the Kuma fixture. Milestone 02 records the measured storage design and budgets.

Use a short record under `docs/decisions/` for a choice that affects several modules: problem, constraints, evidence, choice, rejected alternatives, consequences, affected contracts, and a status line. A proposal to relax scope, retention, security, check frequency, monitor count or latency targets needs Mike's decision; add it to the open decisions in the milestone index.

For a failing gate, report the failing workload, root cause evidence and alternatives that keep the requirements. Mark dependent work gated and continue independent work.

## Testing and integration

- Use small deterministic fixtures for status, uptime and import correctness, and a separate generated workload for capacity.
- Test through real module interfaces with the real embedded engines. Check engines test against local test servers (HTTP, TCP, DNS), not the internet.
- Probe tests run a real agent process against a real server, including server restarts and network cuts.
- Import tests use the synthetic Kuma 2.5.0 fixture in CI. Mike's real database is used locally through `V1PER_KUMA_DB` and never committed.
- Use a controllable clock (a `dev-clock` feature, as in holocron) for interval, retry, maintenance and retention tests.
- Run focused Rust tests and frontend checks for the changed paths, then the export drift check.

Before closing a visual milestone, inspect the changed screens at desktop and phone widths in light and dark modes, exercise keyboard focus, menus, error states and horizontal overflow, and store screenshots with the evidence report. A screenshot does not verify calculations or permissions.

## Completion report

Each task handoff states:

- Behavior delivered and files changed.
- Interface, migration or contract changes.
- Checks run and their outcomes, including failed or unavailable checks.
- Evidence locations and fixture or source revisions.
- Remaining work and the next task that consumes this result.

A milestone closes only when every acceptance criterion has evidence. Set its status and link the evidence in the index. A task with placeholders, disabled required controls, skipped fixtures or unresolved failures stays in progress.

Commits, publishing, production cutover and anything touching a real Kuma installation need Mike's authorization.
