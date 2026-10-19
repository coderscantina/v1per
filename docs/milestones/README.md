# Implementation milestones

Milestone 01 is complete; 02 through 11 and 13 are in progress. Mark one complete only after its acceptance criteria pass and an evidence report is linked here. The [technical briefing](../technical-briefing.md) owns product scope. The [workflow](../development-workflow.md) defines ownership and handoffs.

## Sequence and gates

| ID  | Milestone                                                                  | Dependencies       | Suggested owner                  | Status  |
| --- | -------------------------------------------------------------------------- | ------------------ | -------------------------------- | ------- |
| 01  | [Contracts, fixtures and sources](01-contracts-fixtures-and-sources.md)    | None               | Integration with all owners      | Complete: [evidence](../evidence/m01-contracts-fixtures-and-sources.md) |
| 02  | [Workspace and storage gate](02-workspace-and-storage-gate.md)             | 01                 | Storage and import               | In progress: [evidence](../evidence/m02-storage-gate.md) |
| 03  | [Kit shell and generated API](03-kit-shell-and-generated-api.md)           | 01                 | Frontend and integration         | In progress |
| 04  | [Node tree and access](04-node-tree-and-access.md)                         | 03                 | Integration and frontend         | In progress |
| 05  | [Checks and the built-in probe](05-checks-and-built-in-probe.md)           | 02, 04             | Monitoring and frontend          | In progress |
| 06  | [Remote probe agents](06-remote-probe-agents.md)                           | 05                 | Monitoring                       | In progress |
| 07  | [Notifications](07-notifications.md)                                       | 05                 | Monitoring and frontend          | In progress |
| 08  | [Incidents and maintenance](08-incidents-and-maintenance.md)               | 05, 07             | Monitoring and frontend          | In progress |
| 09  | [Status pages](09-status-pages.md)                                         | 04, 05, 08         | Integration and frontend         | In progress |
| 10  | [Kuma import and cutover](10-kuma-import-and-cutover.md)                   | 05, 07, 08, 09     | Storage and import               | In progress: [evidence](../evidence/m10-kuma-import-and-cutover.md) |
| 11  | [Operations and recovery](11-operations-and-recovery.md)                   | 02, 04; 10 for import state | Storage and import      | In progress: [evidence](../evidence/m11-operations-and-recovery.md) |
| 12  | [Integrated release gate](12-integrated-release-gate.md)                   | 01 through 11      | Integration                      | Pending |
| 13  | [Slim probe agent](13-slim-probe-agent.md)                                 | 06                 | Monitoring and integration       | In progress: [evidence](../evidence/m13-slim-probe-agent.md) |

02 and 03 run in parallel once 01 has settled the protocol envelope and status semantics. The storage gate authorizes the engine design, not a capacity claim; 12 repeats the workload with agents, notifications and pages running.

06, 07 and 08 can run in parallel once 05 has integrated the transition journal. Core authorization lands in 04 and is enforced everywhere after it. Importer inspection and the fixture generator start in 01; mapping waits for the schemas from 05 to 09.

## Open decisions

Decisions waiting for Mike. Remove a line once its record says Adopted.

- Unattended Docker upgrades: `serve` refuses data from an older release unless started with `--migrate` (`V1PER_MIGRATE`) ([0010](../decisions/0010-operations-and-recovery.md)). Recommendation: keep the refusal as the default and document `V1PER_MIGRATE=true` in the compose files as an opt-in, next to a scheduled `v1per backup`.
- Member visibility for clients: a Viewer on `autohaus` currently sees the emails of everyone granted on ancestors (your own staff at the root), as in holocron. Recommendation: show members granted at or below the viewer's highest grant, and only a count for ancestor grants ([capability matrix](../contracts/capability-matrix.md)).
- Node 2FA requirement reach: it applies to grants on the node or below, not to people whose grants sit higher. Recommendation: keep it that way, so a client's requirement does not change your own staff's sign-in; set the requirement at the root for staff.
- Probe offline rule and public bar coloring: proposals in [0008](../decisions/0008-monitoring-rules.md) items 12 and 13.
- Domain expiry alerts: Kuma 2.5.0 also sends domain registration expiry alerts (RDAP lookups), which [0005](../decisions/0005-notifications.md) does not list. 35 of Mike's 69 monitors have them on. Recommendation: add them to the first release as a daily RDAP check per domain in milestone 07, so these alerts do not stop at cutover.
- Release gate targets in the [briefing](../technical-briefing.md) are proposals. Confirm or adjust in 02.
- Minute rollup retention: the agreed default keeps all rollups forever. Measured in 02: 6.3 million minute rows a day, 2.3 billion rows and about 23 GB a year at the target size, and only the 24 h uptime reads them ([m02 evidence](../evidence/m02-storage-gate.md)). Recommendation: keep minute rollups as long as raw heartbeats (90 days), hour and day rollups forever ([0002](../decisions/0002-storage-split-and-retention.md)).
- Kuma HTTP proxies: Mike's database has no proxies (checked 2026-10-18), so this no longer blocks the cutover. Recommendation: leave proxies out of the first release; monitors that use one import paused ([0007](../decisions/0007-kuma-import.md)).

## Known gaps

Found by agents and waiting for a task. Remove a line when fixed.

## Assigning tasks

Split each milestone into interface-sized tasks with explicit file ownership. Acceptance criteria are integrated outcomes, not a file list. Read the referenced contract before changing a shared type or rule. The integration owner resolves overlapping migrations and protocol exports.

Milestone closure includes a focused correctness report, UI evidence where the milestone has screens, updated contracts and decisions, and reproducible commands.
