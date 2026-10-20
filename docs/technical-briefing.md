# V1Per technical briefing

Revision 1. Initial briefing, agreed with Mike on October 18, 2026.

Start with [the documentation index](README.md) for reading order and ownership. This file defines the product and architecture contract. The [monitoring contract](monitoring-contract.md) defines status and uptime semantics. Each milestone has its own brief under [milestones](milestones/README.md). The [decisions](decisions/) record why each choice below was made.

Build a self-hosted Rust server that monitors services, alerts teams and publishes status pages with incident management. It replaces a running Uptime Kuma 2.5.0 installation, takes over its configuration and history, and adds the multi-team structure, probe agents and incident workflow Kuma lacks. Status page and incident ideas come from OpenStatus. Architecture, UI and delivery come from holocron.

This is an implementation brief. Nothing described here is built or measured yet.

**Product contract.** The audience is people who run Uptime Kuma today for their own services and for clients. V1Per runs as one Rust process with a persistent data directory and ships the built Vue assets with the binary. Production needs no Node runtime, external database, queue or extra service. TLS for custom domains terminates at the operator's reverse proxy.

The first release includes:

- a tree of nodes for teams, clients and projects, with inherited grants, an instance admin, 2FA and scoped API keys
- monitors of the types http, keyword, json-query, port, ping, dns, push, group and manual, plus TLS certificate expiry on http monitors
- a built-in probe and remote probe agents with a configurable quorum
- notifications by email (SMTP), Discord, webhook, Slack, Microsoft Teams, Telegram, ntfy and Pushover, with email and Discord at full Kuma feature parity
- manual incidents with an update timeline, auto-incidents from monitors and scheduled maintenance
- status pages with public, login, password, magic link and share token access, custom domains, 90 daily bars, live updates, badges, and English and German
- a one-shot Uptime Kuma 2.5.0 SQLite import with heartbeat history and compatible push URLs

Out of the first release: page subscribers (email, RSS, webhook), Kuma badge URL compatibility, Kuma `/status/:slug` URL compatibility, Prometheus `/metrics`, the Kuma monitor types beyond the list above, Kuma notification providers beyond the core set, hosted SaaS concerns (signup, billing, plans), multi-server operation. Imported objects of unsupported kinds are kept, inventoried and paused, never dropped.

**Architecture references.** Copy and adapt holocron's implementation; do not depend on it. [contracts/architecture-patterns.md](contracts/architecture-patterns.md) describes the patterns in prose: [operation registry](contracts/architecture-patterns.md#operation-registry-and-generated-contracts), [schema export and drift check](contracts/architecture-patterns.md#schema-export-and-drift-check), [commands, revisions and idempotency](contracts/architecture-patterns.md#named-commands-transactions-revisions-and-idempotency), [snapshot queries](contracts/architecture-patterns.md#snapshot-queries), [authorized scope](contracts/architecture-patterns.md#explicit-authorized-scope), [control database boundary](contracts/architecture-patterns.md#control-database-boundary), [topics and subscriptions](contracts/architecture-patterns.md#topics-cursors-and-subscriptions), [typed client](contracts/architecture-patterns.md#typed-client) and [live snapshot adapter](contracts/architecture-patterns.md#live-snapshot-adapter). The Kit and UI references, `docs/kit-implementation.md` and `docs/ux-v2/design.md`, are read in holocron's checkout (`~/Development/products/holocron`) at the pinned revision.

| Holocron pattern                                    | Application in V1Per                                                                                             |
| --------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Axum router, explicit startup composition, AppState | One server hosts the API, probe endpoints, push endpoint, public status pages, health and built web assets       |
| Operation registry with generated TS and OpenAPI    | Named commands and queries in `crates/protocol`; `export-types` generates TypeScript and OpenAPI, drift-checked  |
| Named commands, revisions, idempotency              | Same envelope for monitor, node, page and incident commands                                                      |
| Explicit authorized scope                           | An opaque `NodeScope` replaces holocron's `SiteScope`; obtained by authorization and passed into every operation |
| Control database boundary                           | One SQLite writer, a read pool, WAL, foreign keys, `synchronous=FULL` for the result inbox                       |
| Topics, cursors and SSE subscriptions               | Live monitor state, incidents and import progress; a separate unauthenticated stream per public status page      |
| Durable inbox to DuckDB with a batch ledger         | Probe results are acknowledged after a SQLite commit, then moved to DuckDB in ledgered batches                   |
| Identity: argon2, TOTP, scoped API keys, sealed secrets | Kept; roles move from workspace and site to tree nodes                                                       |
| Kit-based Vue shell, tokens, Vue Query, services    | Admin app; a second light Vite entry for public status pages                                                     |
| CLI subcommands and env configuration               | `init`, `serve`, `probe`, `import kuma`, `backup`, `restore`, `retention`, `diagnostics`; env prefix `V1PER_`    |

Use Rust, Tokio, Axum, SQLx for SQLite and embedded DuckDB through its Rust bindings, at the versions holocron pins unless a newer release is needed. Keep sibling domain modules in one server crate. The probe agent is the same binary in `probe` mode; it must not open DuckDB or the control database.

| Module interface    | Caller receives                                     | Hidden implementation and required invariant                                                                 |
| ------------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Node access         | `NodeScope` or a typed refusal                      | Tree walk, inherited additive grants, instance admin; never discloses nodes outside the scope                |
| Check execution     | A typed check result                                | Per-type check logic, timeouts, retries, TLS inspection; runs identically in the built-in probe and agents   |
| Result admission    | Accepted batch position or retryable refusal        | Validation, durable SQLite commit before acknowledgment, idempotent batch IDs from agents                    |
| Monitor evaluation  | State transitions                                   | Per-probe retries, quorum, maintenance, push deadlines; one open auto-incident per monitor                   |
| Notification outbox | Delivery state per channel and event                | Durable outbox, retries with backoff, resend intervals, provider adapters, templates                         |
| Heartbeat storage   | Bounded typed reads with a watermark                | DuckDB raw heartbeats and rollups, batch ledger, retention, imported Kuma rollups                            |
| Status page reads   | Page snapshot and live deltas for one audience      | Component selection, impact roll-up, daily bars, access mode; never leaks internal detail to public viewers  |
| Kuma import         | Dry-run report or a reconciled destination          | Read-only source, schema signature check, mapping, secret sealing, inventory of everything not mapped        |

Proposed tree:

```text
Cargo.toml
apps/
  server/
    migrations/
    src/
      main.rs  config.rs  state.rs
      http/        (commands, queries, subscribe, probe, push, status pages, web)
      identity/    (accounts, sessions, 2FA, API keys, secrets)
      nodes/       (tree, grants, NodeScope)
      monitors/    (definitions, evaluation, quorum, push deadlines)
      probes/      (built-in probe, agent registry, assignments, agent mode)
      notifications/
      incidents/   (incidents, updates, auto-incidents, maintenance)
      pages/       (status pages, components, access, badges, domains)
      imports/     (kuma)
      maintenance/ (backup, restore, retention, diagnostics)
      background.rs
  web/
    src/
      admin/       (admin app entry)
      status/      (public status page entry)
      api/ generated/ services/ composables/ components/ assets/
crates/
  checks/      (check execution per type, shared by the built-in probe and agents)
  protocol/
  storage/
fixtures/
  kuma-2.5.0/
docs/
sources/manifest.json
```

**Transport surfaces.** Keep these surfaces separate; they differ in authentication, caching and abuse exposure.

| Surface                                   | Required behavior                                                                                               |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `POST /api/commands/{name}`               | Authenticated mutations with typed arguments and results, idempotency and expected revisions                    |
| `GET /api/queries/{name}`                 | Authenticated reads with typed results and snapshot metadata                                                    |
| `GET /api/subscribe`                      | Authenticated SSE topics with durable cursors and resync                                                        |
| `ALL /api/push/{token}`                   | Kuma-compatible push: `status` (`up` or down), `msg` (default `OK`), `ping`; same response shape as Kuma 2.5.0 |
| `/api/probe/*`                            | Agent protocol: assignments with versions and long polling, result batches with batch IDs, agent health        |
| `/s/{slug}` and custom domain roots       | Public status page HTML with injected initial JSON; access checks per page                                      |
| Status page JSON and SSE per page         | Page snapshot and live stream for the audience the request is authorized for                                    |
| Badges                                    | SVG status and uptime badges for public pages and share tokens                                                  |
| Health and runtime config                 | `/health`, boot-time web configuration                                                                          |

**Tenancy and access.** Everything lives in one tree of uniform nodes of any depth, for example `client services > autohaus > schmidt`. Any node can hold monitors, status pages, notification channels, probe agents, incidents, maintenance windows and member grants. A grant gives a role on a node and its whole subtree. Grants only add: the effective role is the highest grant on the node or any ancestor, and there are no deny rules. Roles are holocron's Owner, Admin, Member and Viewer. One instance admin flag covers instance settings, SMTP, shared probes, backups and the root. There are no personal workspaces. A client is a node with invited people. Notification channels and probe agents defined on a node can be used anywhere in its subtree. See [0001](decisions/0001-node-tree-and-access.md).

**Checks and probes.** The server runs a built-in probe so a single box works without setup. Remote agents run `v1per-probe` ([0011](decisions/0011-slim-probe-agent.md)) with a server URL and a token issued on a node, connect over outbound HTTPS only, pull a versioned assignment list with long polling, schedule checks locally, push result batches, and buffer to local disk while the server is unreachable. A monitor runs on one or more probes. It is down when the quorum rule says so: majority by default, with `any` and `all` as per-monitor overrides. Push monitors are evaluated by the server only. See [0003](decisions/0003-probes-checks-and-quorum.md).

**Storage.** SQLite owns accounts, nodes, grants, monitors, probes, channels, pages, incidents, maintenance, current monitor state, the transition journal, the notification outbox, the topic log, import state and the result inbox. DuckDB owns raw heartbeats, minute, hour and day rollups, imported Kuma rollups and the applied-batch ledger. A result is acknowledged to an agent only after a durable SQLite commit; transfer to DuckDB follows holocron's batch ledger protocol so replays never duplicate. Raw heartbeats are kept 90 days. Rollup retention is in [0002](decisions/0002-storage-split-and-retention.md). Do not build a shared repository abstraction over both engines.

**Secrets.** Monitor credentials, notification tokens, SMTP passwords and imported TOTP secrets are sealed with a key from `secrets.key` in the data directory or `V1PER_SECRETS_KEY`. Page passwords are hashed with argon2. Secrets never appear in logs, query results or exports; the UI shows that a secret is set and lets you replace it. See [0006](decisions/0006-secrets-at-rest.md).

**Notifications.** Channels live on nodes. A monitor picks channels from its own node and its ancestors; channels marked default apply to new monitors in their subtree. Events are monitor down, monitor up, certificate expiry and probe offline. Delivery runs from a durable outbox with retries. Kuma's resend interval and custom templates are supported for email and Discord. Email uses the instance SMTP transport unless a channel carries its own, as imported Kuma channels do. See [0005](decisions/0005-notifications.md).

**Incidents and maintenance.** One incident entity serves manual and automatic cases. An incident belongs to a node, names affected monitors and static components with an impact each, and carries an update timeline with the states investigating, identified, monitoring and resolved. A failing monitor opens at most one open auto-incident. Each status page decides whether auto-incidents stay internal drafts until someone publishes them or publish after a grace period. Maintenance windows support Kuma's six strategies, show on pages and suppress alerts. See [0004](decisions/0004-status-pages-and-incidents.md).

**Status pages.** A page lives on a node and shows components picked from monitors in its subtree, plus static components and groups. Access modes are public, node grant login, page password, magic link by email or domain, and share token; a page can allow several restricted modes. Team-internal pages are pages restricted to grant holders. Grant holders also see response-time charts; other viewers do not. A page can claim hostnames and is routed by `Host` header. Components show 90 daily bars in the page timezone, colored by the worst of monitor status, incident impact and maintenance, with the 90-day uptime. Pages use a separate light Vite entry; the server injects the initial JSON into the HTML and the page then follows a live SSE stream. Pages have a default locale and follow the browser among enabled locales (English, German).

**Frontend.** Copy holocron's web app: Vue 3, TypeScript, Vite, bun, Tailwind 4, Reka-based Kit components, oklch tokens, Vue Query for server results, framework-independent services with thin composables, vue-i18n, Vue Router. Holocron's v2 design contract is the visual reference for the admin app: shell, surfaces, geometry, states and copy. Navigation departs from it: the sidebar is one tree of nodes and monitors, the top bar shows the node path and switches between two modes, observe (a live dashboard per node, monitor pages, incidents) and configure (a node's settings as tabs, editors). The node tree replaces holocron's workspace and site context. Status pages reuse the tokens with per-page theming and stay small. Components call generated API resources, never bare `fetch`.

**Kuma import.** `v1per import kuma --source kuma.db --into <node>` reads a Kuma 2.5.0 SQLite file read-only into an empty target node. `--dry-run` reports counts, unsupported types and providers, secrets to be sealed and what will be inventoried. The real run refuses a second import into the same node. Kuma group monitors become child nodes. Raw heartbeats inside the retention window and all `stat_*` rollups go to DuckDB, so imported uptime numbers match Kuma. Push tokens are kept. See [0007](decisions/0007-kuma-import.md).

**Maintenance and security.** Provide `init`, `import kuma`, `backup`, `restore`, `retention` and `diagnostics`. `init` generates bootstrap credentials; there is no default password. Back up SQLite, DuckDB, the inbox, ledger positions, schema versions and the secrets key coherently, starting with a short maintenance pause as holocron does. Rate-limit page passwords, magic link requests and push endpoints. Expose inbox depth and age, DuckDB watermark, check lag per probe, probe health, outbox backlog and failures, and import state.

**Release gate.** Proposed targets for the reference host. Mike confirms or adjusts them in milestone 02 ([open decisions](milestones/README.md#open-decisions)).

| Requirement              | Target                                                                                     |
| ------------------------ | ------------------------------------------------------------------------------------------ |
| Server                   | 2 vCPU, 4 GiB RAM, local SSD                                                               |
| Monitors                 | 2,000: 400 at a 20 s interval and 1,600 at 60 s, about 4 million results a day             |
| Tenancy                  | 50 top-level teams, tree depth up to 6, 10 remote probe agents                             |
| Retention                | 90 days raw heartbeats (about 360 million rows); rollups per [0002](decisions/0002-storage-split-and-retention.md) |
| Result admission         | p95 at most 250 ms per agent batch                                                         |
| Result to status change  | p95 at most 2 s from admission to the updated status on a subscribed page                  |
| Notification dispatch    | p95 at most 5 s from transition to the provider request, provider latency excluded         |
| Status page              | p95 at most 300 ms server time for the initial HTML with 50 components and 90 daily bars   |
| Admin monitor detail     | p95 at most 1 s for a 30-day response-time chart                                           |
| Missed checks            | Under 0.1 % of scheduled checks late by more than one interval at the full workload        |

Test the push endpoint against Kuma 2.5.0 behavior, import of the synthetic fixture and of Mike's real database, quorum with flapping and offline probes, agent buffering across a server restart, inbox interruption at every transfer step, maintenance suppression, page access for every mode, secret sealing, clean-machine backup and restore.

If the gate fails, revise the design using measured evidence. Do not silently lower retention, check frequency, monitor count or latency targets.

## Delivery discipline

The team delivers vertical slices that can be exercised through the generated API and a real Vue screen. A milestone closes when its stated behavior, negative cases and evidence pass. Use [the development workflow](development-workflow.md) for ownership, change requests and handoff reports. Scope changes need Mike's decision.

**Verification status.** Kuma, OpenStatus and holocron findings come from read-only source inspection on October 18, 2026. No code, importer, benchmark or check engine exists.
