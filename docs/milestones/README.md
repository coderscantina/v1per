# Implementation milestones

All milestones are pending. Mark one complete only after its acceptance criteria pass and an evidence report is linked here. The [technical briefing](../technical-briefing.md) owns product scope. The [workflow](../development-workflow.md) defines ownership and handoffs.

## Sequence and gates

| ID  | Milestone                                                                  | Dependencies       | Suggested owner                  | Status  |
| --- | -------------------------------------------------------------------------- | ------------------ | -------------------------------- | ------- |
| 01  | [Contracts, fixtures and sources](01-contracts-fixtures-and-sources.md)    | None               | Integration with all owners      | Pending |
| 02  | [Workspace and storage gate](02-workspace-and-storage-gate.md)             | 01                 | Storage and import               | Pending |
| 03  | [Kit shell and generated API](03-kit-shell-and-generated-api.md)           | 01                 | Frontend and integration         | Pending |
| 04  | [Node tree and access](04-node-tree-and-access.md)                         | 03                 | Integration and frontend         | Pending |
| 05  | [Checks and the built-in probe](05-checks-and-built-in-probe.md)           | 02, 04             | Monitoring and frontend          | Pending |
| 06  | [Remote probe agents](06-remote-probe-agents.md)                           | 05                 | Monitoring                       | Pending |
| 07  | [Notifications](07-notifications.md)                                       | 05                 | Monitoring and frontend          | Pending |
| 08  | [Incidents and maintenance](08-incidents-and-maintenance.md)               | 05, 07             | Monitoring and frontend          | Pending |
| 09  | [Status pages](09-status-pages.md)                                         | 04, 05, 08         | Integration and frontend         | Pending |
| 10  | [Kuma import and cutover](10-kuma-import-and-cutover.md)                   | 05, 07, 08, 09     | Storage and import               | Pending |
| 11  | [Operations and recovery](11-operations-and-recovery.md)                   | 02, 04; 10 for import state | Storage and import      | Pending |
| 12  | [Integrated release gate](12-integrated-release-gate.md)                   | 01 through 11      | Integration                      | Pending |

02 and 03 run in parallel once 01 has settled the protocol envelope and status semantics. The storage gate authorizes the engine design, not a capacity claim; 12 repeats the workload with agents, notifications and pages running.

06, 07 and 08 can run in parallel once 05 has integrated the transition journal. Core authorization lands in 04 and is enforced everywhere after it. Importer inspection and the fixture generator start in 01; mapping waits for the schemas from 05 to 09.

## Open decisions

Decisions waiting for Mike. Remove a line once its record says Adopted.

- Release gate targets in the [briefing](../technical-briefing.md) are proposals. Confirm or adjust in 02.
- Minute rollup retention: the agreed default keeps all rollups forever, which is about a billion minute rows a year at the target size. Recommendation: keep minute rollups as long as raw heartbeats (90 days), hour and day rollups forever ([0002](../decisions/0002-storage-split-and-retention.md)).
- Kuma HTTP proxies: monitors that use a Kuma proxy import paused unless http checks support proxies. Recommendation: support HTTP and SOCKS proxies on http types in 05; it is a small addition and keeps those monitors running ([0007](../decisions/0007-kuma-import.md)).

## Assigning tasks

Split each milestone into interface-sized tasks with explicit file ownership. Acceptance criteria are integrated outcomes, not a file list. Read the referenced contract before changing a shared type or rule. The integration owner resolves overlapping migrations and protocol exports.

Milestone closure includes a focused correctness report, UI evidence where the milestone has screens, updated contracts and decisions, and reproducible commands.
