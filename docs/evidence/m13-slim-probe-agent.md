# Milestone 13 slim probe agent evidence

Measured on 2026-10-19 by the monitoring owner. Design in [0011](../decisions/0011-slim-probe-agent.md).

Verdict: `v1per-probe` is 6.8 MB as a static linux binary and idles at 8.1 MiB resident, about half the memory and a tenth of the stripped size of `v1per probe`. A healthy agent writes no buffer files. The process test with a killed server delivers every result exactly once. The amd64 binary and the CI gate have not run yet; they run on the first CI and release runs.

## Machine and versions

| Item | Value |
| --- | --- |
| Machine | Apple M1 Pro laptop, macOS 27.0.1. Not the reference host |
| Toolchain | rustc 1.99.1; `rust:1.99-alpine` for the linux build |
| Profile | `probe`: release with LTO, one codegen unit, stripped, no debug info |
| Source | commit `ec3e93b` plus the uncommitted milestone 13 work |

## Size

| Binary | Size |
| --- | --- |
| `v1per` release, which held `v1per probe` | 88 MB, 65 MB stripped |
| `v1per-probe`, release profile, macOS | 13 MB, 9.0 MB stripped |
| `v1per-probe`, `probe` profile, macOS | 6.6 MB |
| `v1per-probe`, `probe` profile, `Dockerfile.probe`, linux aarch64 musl, statically linked | 6.8 MB |
| `v1per-probe` image (`FROM scratch`) | 10.3 MB as `docker image ls` reports it |

Leaving `schemars` and `ts-rs` in `v1per-protocol` costs nothing measurable: the linker drops the protocol code the agent does not call.

## Memory

`v1per-probe` (`probe` profile, macOS) against a local `v1per serve`, measured with `ps -o rss=` after the wait.

| Load | Before (`v1per probe`) | After (`v1per-probe`) |
| --- | --- | --- |
| No monitors, 30 s | 16.4 MiB, 4 threads | 8.1 MiB, 2 threads |
| 20 HTTP monitors on loopback at 20 s, plus 3 HTTPS monitors of `example.com` after 60 s | 18.0 MiB with the 20 loopback monitors only | 11.0 MiB |
| Same, server stopped for 25 s | not measured | 11.1 MiB |

## Buffer

| Check | Result |
| --- | --- |
| `buffer/` while the server answered, 23 monitors for 90 s | empty |
| Server stopped for 25 s | one 6.8 kB segment, 23 results waiting |
| Server back for 40 s | segment drained and deleted, `buffered=0` at shutdown |
| `cargo test -p v1per-probe` buffer tests | memory delivery without disk, drain order across a restart with delivered records skipped, oldest segment dropped when full with the count kept across a restart, torn record cut off |

## Tests

| Command | Result |
| --- | --- |
| `cargo test --workspace --no-fail-fast` | all pass, including `apps/probe/tests/agent_process.rs`: agent outbound only, assignment picked up in the long poll, server killed for 30 s, every check delivered exactly once, revoked agent exits 3 with `probe_refused` |
| `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` | clean |
| `bun run typecheck`, `bun run lint`, `bun run test` | clean, 104 tests |
| `bunx playwright test e2e/probes.spec.ts` | 3 passed: the agent started from the command the dialog shows, screens at all sizes, revoked agent stops |
| `docker run v1per-probe` on the scratch image | runs as uid 10001, retries an unreachable server, stops cleanly on SIGTERM |
| `actionlint` on `ci.yml` and `release.yml` | clean |

## Not verified

- The x86_64 musl build and the CI size gate: built only for aarch64 here.
- The release workflow's probe image, binary and tagging jobs: they run on the first release tag.
- Memory at 2,000 monitors every 20 s.
