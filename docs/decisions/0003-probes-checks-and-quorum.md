# 0003: Probes, checks and quorum

Status: Adopted by Mike on 2026-10-18. The probe offline rule is settled in 01. The same-binary agent and its buffer store are superseded by [0011](0011-slim-probe-agent.md).

## Problem

Checks must run from several networks, including client networks without inbound access, while a single-box install keeps working without extra setup.

## Constraints

Self-hosted. Agents may sit behind NAT. 2,000 monitors at intervals down to 20 s. Results must not be lost while the server restarts.

## Choice

- The server runs a built-in probe. Remote agents are the same binary in `probe` mode (superseded by 0011: a separate `v1per-probe` binary). The check and scheduler code is shared.
- Probes are registered on a node with a token shown once and stored hashed. Instance-wide probes live on the root; team probes serve their subtree only.
- Agents connect over outbound HTTPS only. They pull a versioned assignment list with long polling, schedule checks locally with stable jitter, and push result batches with batch IDs that the server admits idempotently.
- Agents buffer results in a bounded local store while the server is unreachable and drain them in order. When the buffer is full the oldest results are dropped and the drop is reported.
- A monitor runs on one or more probes. Its status follows a quorum over live probes: `majority` by default, `any` or `all` per monitor.
- Push monitors are evaluated by the server only.
- The agent never opens the control database or DuckDB.

## Rejected alternatives

- Remote agents only: a single-box install would need an agent before anything works.
- A persistent WebSocket: more connection state for little gain at these intervals.
- A separate slim agent binary: considered; the same binary keeps releases simple. Revisit if the binary size hurts agent deployments. Revisited and adopted in 0011.
- Instance-wide probes only: no way to reach a client's private network for that client alone.

## Consequences

Results carry probe time and receive time; the server records both and flags large clock skew. Agents report their version, and the server refuses protocols it no longer supports with a clear message.

## Affected contracts

Monitoring contract (probe and monitor status), probe protocol types, milestones 05, 06.
