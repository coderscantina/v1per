# Probe protocol

How a remote probe agent and the server talk, how results become monitor status, and how the agent buffers. Decision [0003](../decisions/0003-probes-checks-and-quorum.md) sets the requirements. The code owns the wire types in `apps/server/src/probes/wire.rs`; this page describes them.

## Running an agent

Register a probe with `probes.create` on a node (needs `probes.manage`, so admin and up). The answer shows the token once. Then run the same binary on the other machine:

```sh
V1PER_PROBE_TOKEN=<token> v1per probe --server https://status.example.com --data-dir /var/lib/v1per-probe
```

`--token` works too, but shows in the process list. `--buffer-mb` sets the buffer size (default 64). The agent only opens outbound connections to the server; it listens on nothing, so it works behind NAT with outbound HTTPS alone. It never opens `control.sqlite` or DuckDB. One agent at a time uses a data directory.

A probe on a node serves monitors on that node and below. Instance-wide probes, the built-in one included, live on the root. `probes.revoke` ends a token at once: the agent's next request is refused, it exits with status 3 and the message, and the probe leaves every monitor it served.

## Requests

Every request is a JSON `POST` with two headers:

- `Authorization: Bearer <token>`
- `x-v1per-probe-protocol: 1`

Times are Unix milliseconds. IDs are decimal strings, as in the admin API.

### `POST /api/probe/assignments`

A long poll for the agent's assignment list, which also reports its health.

```json
{
  "version": "9f2c…",
  "sent_at": 1790812800000,
  "health": {
    "version": "0.1.0",
    "checks_started": 1200,
    "checks_late": 0,
    "buffered_results": 0,
    "dropped_results": 0
  }
}
```

`version` is the list version the agent runs, or `null` at start. When the server's list differs, it answers at once with the whole list; otherwise it waits up to 25 seconds for a change and answers `"assignments": null` with the same version. The agent polls again right away.

```json
{
  "version": "4b81…",
  "assignments": [
    {
      "monitor": "42",
      "revision": 7,
      "interval_ms": 60000,
      "retry_interval_ms": 20000,
      "max_retries": 2,
      "down_on_query_mismatch": false,
      "updated_at": 1790812700000,
      "definition": { "type": "http", "url": "https://shop.example.com/health", "timeout": 48, "...": "..." }
    }
  ]
}
```

The version is a digest of the list, so it survives server restarts. `definition` is the check as `crates/checks` runs it, with credentials unsealed. They travel only in answers to the probe's own authenticated requests, and the agent keeps them in memory only: an agent started while the server is unreachable checks nothing until it gets its list.

### `POST /api/probe/results`

One batch of results, each after the probe's own retries.

```json
{
  "batch": "6f0c2b1e-…-381",
  "sent_at": 1790812861000,
  "results": [
    {
      "monitor": "42",
      "revision": 7,
      "time": 1790812860000,
      "status": "down",
      "retries": 3,
      "latency_ms": null,
      "message": "Request failed with status code 503",
      "http_status": 503,
      "cert_expiry": 1798588800000
    }
  ]
}
```

`batch` is unique per probe, at most 128 characters, and names the same results on every resend. A batch holds at most 1,000 results and 4 MiB. `status` is `down`, `up` or `pending`. The server answers only after a durable SQLite commit of the heartbeats together with the monitor-level statuses evaluated from them:

```json
{ "batch": "6f0c2b1e-…-381", "repeated": false }
```

A resent batch answers `"repeated": true` and stores nothing new. The repeat check compares the heartbeats only, as the server may derive other monitor-level statuses for the same results. Results for a monitor the probe no longer runs, or of another revision, are dropped.

### Refusals

Every refusal has a stable `error` and a `message` for the agent's operator.

| Status | `error`                | Agent does                                                 |
| ------ | ---------------------- | ---------------------------------------------------------- |
| 401    | `unauthenticated`      | Stops: the token is unknown or revoked                     |
| 426    | `protocol_unsupported` | Stops; the message names both protocol versions            |
| 429    | `rate_limited`         | Retries; more than 30 unknown tokens a minute per address  |
| 503    | `unavailable`          | Retries                                                    |
| 400    | `malformed_request`    | Drops the batch and logs it                                |
| 409    | `batch_conflict`       | Drops the batch and logs it: the ID was used for other results |
| 413    | `payload_too_large`    | Drops the batch and logs it                                |

Retries wait 1 second, doubling up to 30 seconds. The server speaks exactly one protocol version; an agent from another release is refused with the message rather than misread.

## Clocks and health

Each request carries `sent_at` on the agent's clock. The server records `clock_skew_ms`, its own clock minus `sent_at`, and flags the probe when the difference is above 5 seconds. Heartbeats keep the probe's own `time`; `probe_state` keeps both that time and the server's receive time, and liveness is read from the receive time, so a skewed agent still counts correctly.

`probes.list` shows each probe's status (`waiting`, `online`, `offline`, `revoked`), last contact, version, skew, checks started and started late, results buffered and dropped, and its monitor count. A probe without contact for 90 seconds (three missed polls) goes offline; the probe journal `probe_transitions` records offline and back online, and `probe.offline` and `probe.online` go to the probe node's `probes:<id>` topic. Notifications (milestone 07) read the journal.

## Scheduling and retries

Agents run the same scheduler as the built-in probe (`apps/server/src/probes/schedule.rs`): a fixed grid per monitor, with a phase derived from the monitor ID, so every probe checks a monitor in the same slot. A monitor changed since the probe started is checked at once. Each probe applies Kuma's retry rule to its own results (decision 0008): pending while `retries < max_retries`, then down, the next check after `retry_interval_ms` while pending. An agent starts each monitor's count at zero when it starts or the monitor changes; the built-in probe continues from its stored count.

## Rounds and quorum

A monitor runs on the probes it names, the built-in probe by default, and only probes registered on its node or an ancestor. Its status follows the quorum over live probes: `majority` (default) is down when at least half are down, `any` when one is, `all` when all are; pending follows the same rule over down and pending probes.

A probe is live for a monitor while it is online, not revoked, still assigned, and its latest result arrived within three of the monitor's intervals (or retry intervals, whichever is longer). The first three intervals after the server starts count as recent, so a restart does not drop every probe at once.

Several probes would otherwise multiply the monitor-level series. The rule:

- Every current result updates its probe's state and the quorum.
- A monitor-level beat, with its entry in the monitor-level series, is recorded for every result of the lead probe, the live probe with the lowest ID, and for any result that changes the monitor's status.

So the series keeps one entry per lead round, as Kuma keeps one beat per check, uptime is not inflated by the probe count, and a change never waits for the lead. A status change adds one extra entry in its round.

A result that waited at the agent for longer than one interval (`sent_at - time`) is history only: its heartbeat counts in the probe's own series, but it changes neither the probe's state nor the monitor's. After an outage the monitor-level series therefore has a gap while the agent drains, rather than a guess at the past quorum.

With no live probe the monitor is pending with the message "No live probe". This is recorded once, as a state change without a series entry, since nothing was checked.

## Buffer

The agent seals the results of each second, at most 1,000, into one batch in `buffer.sqlite` in its data directory, and delivers batches oldest first, each until the server acknowledges it. Batch IDs are `<buffer id>-<sequence>`, where the buffer ID is random per buffer file, so a new buffer never reuses an old ID.

A small SQLite file rather than an append log: dropping the oldest batch, removing a delivered one and surviving a crash mid-write are single transactions instead of file rewrites and recovery code, and SQLx is already in the binary. It runs WAL with `synchronous=NORMAL`: an agent crash loses nothing sealed, a power loss may lose the last commits, and results not yet sealed (under a second) are lost with the process.

When the buffer passes its size, the oldest whole batches are dropped and counted. The count is kept in the buffer file and reported as `dropped_results` with every poll. A batch never changes once sealed, so a resend after a lost acknowledgment is always recognized as a repeat.
