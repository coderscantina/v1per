# 0011: Slim probe agent

Status: Adopted by Mike on 2026-10-19. Supersedes the same-binary and SQLite buffer parts of [0003](0003-probes-checks-and-quorum.md).

## Problem

Remote agents ran as `v1per probe` inside the server binary: 88 MB, 65 MB stripped, about 16 MiB resident while idle. DuckDB, the web server, mail and the Kuma importer came along although the agent never runs them. Agents go on small VMs, office machines and Raspberry Pis inside client networks, where a large image and steady disk writes cost more than on the server.

## Constraints

Every check type the built-in probe runs must run on an agent. The protocol, the quorum and the no-loss, no-duplicate rule after a server outage stay as 0003 and the [probe protocol](../contracts/probe-protocol.md) set them. Agents run the same release as the server.

## Choice

- A separate crate, `apps/probe`, builds the `v1per-probe` binary. `v1per probe` is removed. The flags and `V1PER_PROBE_*` variables stay the same.
- `crates/probe-core` holds what both probes share: the wire types, the scheduler, Kuma's retry rule and the clock. It depends on `v1per-checks` and `v1per-protocol`, not on storage. The server's built-in probe uses it too.
- The agent runs a current-thread Tokio runtime.
- The buffer is an append-only segment log in `buffer/` in the data directory. Batches wait in memory and spill to it only while the server is unreachable or more than about ten seconds of results wait. Each spill is synced. Delivered records are marked in place, and a segment is deleted once fully delivered. Over `--buffer-mb` (64 by default) the oldest segment is dropped and counted. A torn record is cut off at start by its length and checksum.
- An existing `buffer.sqlite` is left alone with a warning. Operators let the old agent drain before upgrading.
- Releases ship a static musl binary for linux amd64 and arm64 and an image built `FROM scratch`, `ghcr.io/coderscantina/v1per-probe`, without a healthcheck. The Probes page and the probe offline alert are the liveness signal.
- Targets: at most 15 MB stripped and 16 MiB resident while idle. CI fails a probe binary over 15 MB. Resident memory is measured for the evidence, not gated.

## Rejected alternatives

- A cargo feature on the server crate: cfg gates through `main.rs` and the config, and server dependencies creep back in unnoticed.
- Keeping SQLite through rusqlite: proven, but brings C code back and writes every batch while the server is healthy.
- Writing every batch through the file, as before: nothing sealed is lost in an agent crash, but an SD card gets a write every second for nothing. Now a crash loses the undelivered results of the last seconds at most.
- A `codegen` feature to keep `schemars` and `ts-rs` out of the agent: measured unnecessary. The linker drops the unused protocol code, and the agent is 6.6 MB without it.
- Dropping heavy check types (JSON query, DNS) from agents: monitors would get probe-specific limits.

## Consequences

Two binaries and two images per release. The release workflow builds the probe image and binaries next to the server image. The agent's process test lives in `apps/probe/tests` and runs the `v1per` server binary from the same build.

## Affected contracts

Probe protocol (running an agent, buffer), probe guide, configuration, install guide, milestone 13.
