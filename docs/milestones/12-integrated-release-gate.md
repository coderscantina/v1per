# Milestone 12: integrated release gate

## Outcome

The complete product meets the release gate in the [technical briefing](../technical-briefing.md) on the reference host, with Mike's Kuma data imported and running.

## Dependencies and ownership

Requires 01 through 11. Integration owns the gate run and report; every owner fixes failures in their modules.

## Work

1. Run the release workload with ten agents, notifications to local sinks, 50 open status pages and two admin users, for at least 24 hours of simulated time and one hour of real time at full rate.
2. Measure every gate target and missed checks.
3. Repeat the failure cases: server restart, agent network cuts, provider outages, disk pressure, backup and restore during load.
4. Rehearse the cutover from Mike's Kuma database end to end.
5. Close every acceptance criterion left open in earlier milestones.

## Acceptance criteria

- Every gate target passes, or a decision adopted by Mike changes it.
- No duplicate or lost heartbeat, notification or incident update in any failure case.
- The cutover rehearsal ends with matching uptime numbers and working alerts.

## Evidence and handoff

`docs/evidence/m12-integrated-release-gate.md` with machine, versions, workload, results and anything not verified. Release requires Mike's approval.
