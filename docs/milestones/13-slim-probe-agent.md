# Milestone 13: Slim probe agent

## Outcome

An operator runs `v1per-probe`, a static binary of at most 15 MB, or its `FROM scratch` image, on a small host inside a client network. It checks everything the built-in probe checks, writes nothing to disk while the server answers, and keeps results in a bounded segment log through a server outage, per [0011](../decisions/0011-slim-probe-agent.md).

## Dependencies and ownership

Requires 06. Monitoring owns `apps/probe`, `crates/probe-core` and the agent's tests. Integration owns the Dockerfiles, CI and the release workflow. Frontend owns the agent command the Add probe dialog shows.

## Work

1. Move the wire types, scheduler, retry rule and clock into `crates/probe-core`; the server's built-in probe uses them from there.
2. Build `v1per-probe` in `apps/probe` on a current-thread runtime and remove `v1per probe`.
3. Replace `buffer.sqlite` with the segment log: memory first, spill while offline or behind, synced spills, delivered marks in place, drop the oldest segment over the size, cut off torn records, warn about a leftover `buffer.sqlite`.
4. Add the `probe` cargo profile, `Dockerfile.probe`, the CI size gate, and the release jobs for the probe image and the linux amd64 and arm64 binaries.
5. Update the probe protocol, guides, compose file, e2e journey and the command the admin app shows.

## Acceptance criteria

- The static musl binary is at most 15 MB, and CI fails above it.
- Idle resident memory is at most 16 MiB.
- A healthy agent creates no buffer files.
- Killing the server loses no results and duplicates none after it returns (`apps/probe/tests/agent_process.rs`).
- A revoked agent exits with status 3 and the reason.
- Buffer tests cover the drain order across a restart, dropping the oldest segment when full, and a torn record.

## Evidence and handoff

Sizes, memory and test runs in [m13 evidence](../evidence/m13-slim-probe-agent.md).
