# v1per development handoff

Build a Rust uptime monitoring and status page server for teams that run Uptime Kuma today. v1per imports an Uptime Kuma 2.5.0 SQLite database with its history, runs checks from a built-in probe and remote probe agents, and publishes status pages with incident management. One instance serves many teams and clients through a tree of nodes.

The server, admin app, probe agent, Kuma import and operator commands are built, and v1per is heading for its first public beta. The architecture, UI and development flow are taken from holocron, Mike's Rust analytics app, and adapted here.

To install, run or use v1per, read the [user guide](guide/README.md). The rest of this index is for people who work on v1per.

## Read in this order

1. [Technical briefing](technical-briefing.md): agreed scope, architecture, compatibility and release requirements.
2. [Monitoring contract](monitoring-contract.md): status values, uptime math, incident impact and the rules still to settle in milestone 01.
3. [Milestone index](milestones/README.md): dependency order, parallel work and the individual milestone briefs.
4. [Development workflow](development-workflow.md): ownership, shared interfaces, decisions and completion evidence.
5. [Decisions](decisions/): the records behind the choices in the briefing. Read the ones your milestone links.

The technical briefing owns product requirements. The monitoring contract owns status and uptime semantics. Milestone files refer to these sources rather than restating them.

## Requirements that drive the design

| Constraint                                  | Consequence                                                                                                                       |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| Replace a running Uptime Kuma 2.5.0         | One-shot import of config, secrets, raw heartbeats and `stat_*` rollups. Push URLs keep working. Imported uptime numbers match.   |
| Self-hosted, one process, one data directory | Rust serves the built Vue assets. SQLite holds control data, embedded DuckDB holds heartbeats. No Node, external database or queue. |
| Many teams and clients on one instance       | A tree of uniform nodes with inherited, additive grants. Every read and write goes through an authorized node scope.               |
| Checks from several networks                 | The server has a built-in probe; remote agents run the same binary over outbound HTTPS. Monitor status uses a probe quorum.       |
| Status pages for public, clients and teams   | Pages pick components from their node's subtree and support public, login, password, magic link and share token access.          |
| Holocron is the template                     | Copy and adapt its workspace layout, protocol registry, identity, storage patterns, Kit-based UI and doc-driven delivery.         |
| Small reference machine                      | Prove 2,000 monitors on 2 vCPU and 4 GiB RAM with 90 days of raw heartbeats.                                                      |

## Team starting point

Assign an integration owner, a monitoring owner, a storage and import owner, and a frontend owner. These are responsibilities, not four people. Give each active task explicit file ownership and an interface contract. The integration owner coordinates protocol generation, migrations order and gate evidence.

Begin with [milestone 01](milestones/01-contracts-fixtures-and-sources.md). Once its contracts are stable, milestones 02 and 03 can run in parallel.

## References

Pinned in [sources/manifest.json](../sources/manifest.json):

- **holocron** at `84fc54ec`: the architecture, UI and workflow template. It is Mike's private repository and not distributable. Agents on Mike's machine read its Kit and UI documents (`docs/kit-implementation.md`, `docs/ux-v2/design.md`) at `~/Development/products/holocron`. The architecture patterns are ported into [contracts/architecture-patterns.md](contracts/architecture-patterns.md). Code copied from it into v1per is relicensed by Mike under AGPL-3.0; keep the Kit notices that holocron keeps.
- **Uptime Kuma 2.5.0**: the import source and the push URL compatibility target (MIT).
- **OpenStatus**: a model reference for status pages and incidents (AGPL-3.0). Borrow ideas, not code.

## Evidence and status

Keep milestone status and evidence links in the milestone index. Store result reports under `docs/evidence/`, named `mNN-<slug>.md`, with screenshots in `docs/evidence/mNN/`. Keep real Kuma databases, secrets and large workloads outside Git. A report states source revision, command, fixture, machine, engine versions, result and what was not verified.

Milestone 01 is complete, 02 through 11 are in progress and 12 is pending; see the [milestone index](milestones/README.md).
