# Milestone 06: remote probe agents

## Outcome

An admin registers a probe on a node, runs `v1per probe --server <url> --token <token>` on another machine, assigns monitors to it, and the monitor status follows the quorum across all its probes.

## Dependencies and ownership

Requires 05. Monitoring owns `probes`, the agent mode and the `/api/probe/*` routes. Frontend owns the probe screens and the per-probe status in monitor detail.

## Work

1. Implement probe registration per [0003](../decisions/0003-probes-checks-and-quorum.md): tokens issued on a node, shown once, stored hashed, revocable. Instance-wide probes live on the root.
2. Implement the agent protocol: versioned assignment list with long polling, result batches with batch IDs admitted idempotently, agent health and version reporting.
3. Run the same check and scheduler code in agent mode. Buffer results to a bounded local file store while the server is unreachable and drain it in order.
4. Implement quorum `majority`, `any` and `all`, the offline rule from the settled contract, and the probe offline event.
5. Build the UI: probes per node with health and lag, assignment of probes to monitors (only probes visible from the monitor's node), per-probe status and latency on monitor detail.

## Acceptance criteria

- An agent behind NAT with only outbound HTTPS works.
- Killing the server for ten minutes loses no results and duplicates none after it returns.
- Quorum tests cover flapping probes, one probe offline, all probes offline and a probe revoked mid-run.
- A team-owned probe is never offered to monitors outside its subtree.

## Evidence and handoff

Network-cut and restart tests and screenshots in `docs/evidence/m06-remote-probe-agents.md`. Hand probe offline events to 07.
